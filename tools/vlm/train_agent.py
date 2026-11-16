import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import overlap
from agent_format import assistant_labels, render, special_ids
from prompt_format import BASE

IMAGE_SIDE = 256


def load_examples(path):
    overlap.refuse_minds_eye(path)
    return [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]


def encode(example, processor, max_tokens, side=IMAGE_SIDE):
    from PIL import Image

    text, paths = render(example)
    images = [Image.open(p).convert("RGB").resize((side, side)) for p in paths]
    batch = processor(text=[text], images=images or None, return_tensors="pt")
    ids = batch["input_ids"][0].tolist()
    if len(ids) > max_tokens:
        return None
    import torch

    start_id, end_id, assistant_id = special_ids(processor.tokenizer)
    batch["labels"] = torch.tensor([assistant_labels(ids, start_id, end_id, assistant_id)])
    return batch


def main():
    import argparse
    import random
    import time

    import torch
    from peft import PeftModel
    from transformers import AutoModelForImageTextToText, AutoProcessor

    parser = argparse.ArgumentParser()
    parser.add_argument("--data", required=True)
    parser.add_argument("--adapter", default="/home/kieran/gcad-datasets/out/vlm/lora_qwen35")
    parser.add_argument("--out", required=True)
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--steps", type=int, default=500)
    parser.add_argument("--accum", type=int, default=8)
    parser.add_argument("--lr", type=float, default=5e-5)
    parser.add_argument("--max-tokens", type=int, default=12000)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    random.seed(args.seed)
    torch.manual_seed(args.seed)
    examples = load_examples(args.data)
    processor = AutoProcessor.from_pretrained(args.base)
    model = AutoModelForImageTextToText.from_pretrained(args.base, torch_dtype=torch.bfloat16, device_map={"": 0})
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    model = PeftModel.from_pretrained(model, args.adapter, is_trainable=True)
    optimizer = torch.optim.AdamW([p for p in model.parameters() if p.requires_grad], lr=args.lr, weight_decay=0.0)
    model.train()
    log = open(Path(args.out).parent / f"agent_sft_log_{Path(args.out).name}.jsonl", "a")
    started = time.time()
    for step in range(args.steps):
        total = 0.0
        for _ in range(args.accum):
            batch = None
            while batch is None:
                batch = encode(random.choice(examples), processor, args.max_tokens)
            batch = {k: v.to(model.device) for k, v in batch.items()}
            loss = model(**batch).loss / args.accum
            loss.backward()
            total += loss.item()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        optimizer.zero_grad(set_to_none=True)
        log.write(json.dumps({"step": step + 1, "loss": total, "seconds": round(time.time() - started, 1)}) + "\n")
        log.flush()
    model.save_pretrained(args.out)
    processor.save_pretrained(args.out)


if __name__ == "__main__":
    main()
