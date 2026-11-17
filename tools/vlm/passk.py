import argparse
import json
import sys
from pathlib import Path

import torch
from peft import PeftModel
from PIL import Image
from transformers import AutoModelForImageTextToText, AutoProcessor

sys.path.insert(0, str(Path(__file__).resolve().parent))

from briefs import styled
from eval import IMAGE_SIDE
from pairs_data import DATA, VIEWS, read_pairs, row_prompt
from prompt_format import BASE, completion_tokens, format_for, prompt_text, stop_ids
from repair import build_error
from reward import strip_fences


def inputs(processor, fmt, row, brief):
    if brief:
        return processor(text=[prompt_text(processor, fmt, 0, styled(brief, row["id"]))], return_tensors="pt")
    images = [Image.open(DATA / row["renders"][v]).convert("RGB").resize((IMAGE_SIDE, IMAGE_SIDE)) for v in VIEWS]
    return processor(text=[prompt_text(processor, fmt, len(images), row_prompt(row))], images=images, return_tensors="pt")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adapter", required=True)
    parser.add_argument("--results", required=True)
    parser.add_argument("--briefs", default="")
    parser.add_argument("--k", type=int, default=8)
    parser.add_argument("--temperature", type=float, default=0.8)
    parser.add_argument("--max-new-tokens", type=int, default=700)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    failed = {json.loads(l)["id"] for l in Path(args.results).read_text().splitlines() if l.strip() and not json.loads(l)["built"]}
    briefs = {}
    if args.briefs:
        briefs = {j["id"]: j["brief"] for j in map(json.loads, Path(args.briefs).read_text().splitlines()) if j.get("brief")}
    rows = [r for r in read_pairs(split="test") if r["id"] in failed]
    fmt = format_for(BASE)
    processor = AutoProcessor.from_pretrained(BASE)
    stop = stop_ids(processor.tokenizer, fmt)[0]
    pad = processor.tokenizer.pad_token_id
    model = AutoModelForImageTextToText.from_pretrained(BASE, dtype=torch.bfloat16, device_map={"": 0})
    model = PeftModel.from_pretrained(model, args.adapter)
    model.eval()
    hits = 0
    with open(args.out, "w") as f:
        for row in rows:
            batch = inputs(processor, fmt, row, briefs.get(row["id"]) if args.briefs else None).to(model.device)
            with torch.no_grad():
                out = model.generate(**batch, do_sample=True, temperature=args.temperature, max_new_tokens=args.max_new_tokens,
                                     num_return_sequences=args.k, eos_token_id=[stop], pad_token_id=pad if pad is not None else stop)
            comps = [completion_tokens(c.tolist(), stop, pad) for c in out[:, batch["input_ids"].shape[1]:]]
            texts = processor.tokenizer.batch_decode(comps, skip_special_tokens=True)
            errors = [build_error(strip_fences(t)) for t in texts]
            built = sum(e is None for e in errors)
            hits += built > 0
            f.write(json.dumps({"id": row["id"], "built": built, "k": args.k, "errors": [e and e[:160] for e in errors], "programs": texts}) + "\n")
            f.flush()
    print(json.dumps({"failed_prompts": len(rows), "any_built": hits, "k": args.k}))


if __name__ == "__main__":
    main()
