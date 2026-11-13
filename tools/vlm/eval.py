import argparse
import json
import statistics
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import torch
from peft import PeftModel
from PIL import Image
from transformers import AutoModelForImageTextToText, AutoProcessor

sys.path.insert(0, str(Path(__file__).resolve().parent))

from pairs_data import DATA, PROMPT, VIEWS, read_pairs
from reward import score

BASE = "Qwen/Qwen3-VL-4B-Instruct"
IMAGE_SIDE = 256


def reference_mesh(row):
    return DATA / "out" / "zero-to-cad" / "glb" / row["id"].split(":")[1][:2] / (row["id"].split(":")[1] + ".glb")


def generate(model, processor, row, max_new_tokens, temperature):
    images = [Image.open(DATA / row["renders"][v]).convert("RGB").resize((IMAGE_SIDE, IMAGE_SIDE)) for v in VIEWS]
    content = [{"type": "image"} for _ in images] + [{"type": "text", "text": PROMPT}]
    prompt = processor.apply_chat_template([{"role": "user", "content": content}], tokenize=False, add_generation_prompt=True)
    batch = processor(text=[prompt], images=images, return_tensors="pt").to(model.device)
    kwargs = {"max_new_tokens": max_new_tokens, "do_sample": temperature > 0}
    if temperature > 0:
        kwargs["temperature"] = temperature
    with torch.no_grad():
        out = model.generate(**batch, **kwargs)
    return processor.tokenizer.decode(out[0, batch["input_ids"].shape[1] :], skip_special_tokens=True)


def summarise(results):
    built = [r for r in results if r["built"]]
    good = [r for r in built if r["metrics"]["volume_error"] <= 0.01 and r["metrics"]["chamfer"] <= 0.005]
    return {
        "n": len(results),
        "build_rate": len(built) / max(1, len(results)),
        "match_rate": len(good) / max(1, len(results)),
        "mean_reward": statistics.fmean(r["reward"] for r in results) if results else 0.0,
        "median_chamfer": statistics.median(r["metrics"]["chamfer"] for r in built) if built else None,
        "median_volume_error": statistics.median(r["metrics"]["volume_error"] for r in built) if built else None,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adapter", default="")
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--split", default="test")
    parser.add_argument("--limit", type=int, default=200)
    parser.add_argument("--max-new-tokens", type=int, default=700)
    parser.add_argument("--temperature", type=float, default=0.0)
    parser.add_argument("--max-lines", type=int, default=60)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    processor = AutoProcessor.from_pretrained(args.base)
    model = AutoModelForImageTextToText.from_pretrained(args.base, dtype=torch.bfloat16, device_map={"": 0})
    if args.adapter:
        model = PeftModel.from_pretrained(model, args.adapter)
    model.eval()
    rows = read_pairs(split=args.split, max_lines=args.max_lines)[: args.limit]
    results = []
    with ThreadPoolExecutor(4) as pool, open(args.out, "w") as f:
        pending = []
        for row in rows:
            text = generate(model, processor, row, args.max_new_tokens, args.temperature)
            pending.append((row, text, pool.submit(score, text, reference_mesh(row))))
        for row, text, future in pending:
            r = future.result()
            r.update({"id": row["id"], "program": text})
            results.append(r)
            f.write(json.dumps(r) + "\n")
    print(json.dumps(summarise(results)))


if __name__ == "__main__":
    main()
