import argparse
import json
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import torch
from peft import PeftModel
from transformers import AutoModelForImageTextToText, AutoProcessor

sys.path.insert(0, str(Path(__file__).resolve().parent))

from briefs import styled
from eval import reference_mesh, summarise
from pairs_data import DATA, read_pairs
from prompt_format import BASE, format_for, prompt_text, stop_ids
from reward import score


def generate_text(model, processor, question, max_new_tokens, fmt):
    prompt = prompt_text(processor, fmt, 0, question)
    batch = processor(text=[prompt], return_tensors="pt").to(model.device)
    kwargs = {"max_new_tokens": max_new_tokens, "do_sample": False}
    if fmt == "plain":
        kwargs["eos_token_id"] = stop_ids(processor.tokenizer, fmt)
    with torch.no_grad():
        out = model.generate(**batch, **kwargs)
    return processor.tokenizer.decode(out[0, batch["input_ids"].shape[1]:], skip_special_tokens=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adapter", default="")
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--briefs", default=str(DATA / "out" / "vlm_brief" / "briefs.jsonl"))
    parser.add_argument("--split", default="test")
    parser.add_argument("--limit", type=int, default=100)
    parser.add_argument("--offset", type=int, default=0)
    parser.add_argument("--max-new-tokens", type=int, default=700)
    parser.add_argument("--max-lines", type=int, default=60)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    briefs = {j["id"]: j["brief"] for j in map(json.loads, Path(args.briefs).read_text().splitlines()) if j.get("brief")}
    rows = [r for r in read_pairs(split=args.split, max_lines=args.max_lines) if r["id"] in briefs][args.offset : args.offset + args.limit]
    fmt = format_for(args.base)
    processor = AutoProcessor.from_pretrained(args.base)
    model = AutoModelForImageTextToText.from_pretrained(args.base, dtype=torch.bfloat16, device_map={"": 0})
    if args.adapter:
        model = PeftModel.from_pretrained(model, args.adapter)
    model.eval()
    results = []
    with ThreadPoolExecutor(4) as pool, open(args.out, "w") as f:
        pending = []
        for row in rows:
            text = generate_text(model, processor, styled(briefs[row["id"]], row["id"]), args.max_new_tokens, fmt)
            pending.append((row, text, pool.submit(score, text, reference_mesh(row))))
        for row, text, future in pending:
            r = future.result()
            r.update({"id": row["id"], "program": text})
            results.append(r)
            f.write(json.dumps(r) + "\n")
    print(json.dumps(summarise(results)))


if __name__ == "__main__":
    main()
