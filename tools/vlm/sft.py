import argparse
import json
import random
import time
from pathlib import Path

import torch
from peft import LoraConfig, get_peft_model
from PIL import Image
from transformers import AutoModelForImageTextToText, AutoProcessor

BASE = "Qwen/Qwen3-VL-4B-Instruct"
IMAGE_SIDE = 256


def load_image(path):
    return Image.open(path).convert("RGB").resize((IMAGE_SIDE, IMAGE_SIDE))


def to_batch(processor, examples, device):
    texts, images = [], []
    for ex in examples:
        msgs = []
        for m in ex["messages"]:
            content = []
            for c in m["content"]:
                if c["type"] == "image":
                    images.append(load_image(c["image"]))
                    content.append({"type": "image"})
                else:
                    content.append(c)
            msgs.append({"role": m["role"], "content": content})
        texts.append(msgs)
    prompts = [processor.apply_chat_template(m[:1], tokenize=False, add_generation_prompt=True) for m in texts]
    fulls = [p + m[1]["content"][0]["text"] + "<|im_end|>" for p, m in zip(prompts, texts)]
    batch = processor(text=fulls, images=images, return_tensors="pt", padding=True)
    prompt_batch = processor(text=prompts, images=images, return_tensors="pt", padding=True)
    labels = batch["input_ids"].clone()
    labels[batch["attention_mask"] == 0] = -100
    side = processor.tokenizer.padding_side
    for i in range(len(examples)):
        n = int(prompt_batch["attention_mask"][i].sum())
        if side == "left":
            pad = int((batch["attention_mask"][i] == 0).sum())
            labels[i, : pad + n] = -100
        else:
            labels[i, :n] = -100
    batch["labels"] = labels
    return {k: v.to(device) for k, v in batch.items()}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", default="/home/kieran/gcad-datasets/out/vlm")
    parser.add_argument("--out", default="/home/kieran/gcad-datasets/out/vlm/lora")
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--steps", type=int, default=2000)
    parser.add_argument("--batch", type=int, default=4)
    parser.add_argument("--accum", type=int, default=4)
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--rank", type=int, default=32)
    parser.add_argument("--save-every", type=int, default=500)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    random.seed(args.seed)
    torch.manual_seed(args.seed)
    processor = AutoProcessor.from_pretrained(args.base)
    processor.tokenizer.padding_side = "right"
    model = AutoModelForImageTextToText.from_pretrained(args.base, torch_dtype=torch.bfloat16, device_map={"": 0})
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    lora = LoraConfig(
        r=args.rank,
        lora_alpha=args.rank * 2,
        lora_dropout=0.0,
        target_modules=r".*language_model.*\.(q_proj|k_proj|v_proj|o_proj|gate_proj|up_proj|down_proj)",
        task_type="CAUSAL_LM",
    )
    model = get_peft_model(model, lora)
    model.print_trainable_parameters()

    examples = [json.loads(l) for l in open(Path(args.data) / "train.jsonl")]
    optimizer = torch.optim.AdamW([p for p in model.parameters() if p.requires_grad], lr=args.lr, weight_decay=0.0)
    total = args.steps
    schedule = torch.optim.lr_scheduler.LambdaLR(
        optimizer, lambda s: min(1.0, (s + 1) / 30) * max(0.05, 1 - s / total)
    )
    model.train()
    order = []
    log = open(Path(args.out).parent / "sft_log.jsonl", "a")
    started = time.time()
    for step in range(total):
        loss_sum = 0.0
        for _ in range(args.accum):
            if len(order) < args.batch:
                order = random.sample(range(len(examples)), len(examples))
            idx, order = order[: args.batch], order[args.batch :]
            batch = to_batch(processor, [examples[i] for i in idx], model.device)
            loss = model(**batch).loss / args.accum
            loss.backward()
            loss_sum += loss.item()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        schedule.step()
        optimizer.zero_grad(set_to_none=True)
        record = {"step": step + 1, "loss": loss_sum, "seconds": round(time.time() - started, 1)}
        log.write(json.dumps(record) + "\n")
        log.flush()
        if (step + 1) % 10 == 0:
            print(record, flush=True)
        if (step + 1) % args.save_every == 0 or step + 1 == total:
            model.save_pretrained(args.out)
    processor.save_pretrained(args.out)


if __name__ == "__main__":
    main()
