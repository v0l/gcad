import argparse
import json
import random
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import torch
from peft import PeftModel
from PIL import Image
from transformers import AutoModelForImageTextToText, AutoProcessor

sys.path.insert(0, str(Path(__file__).resolve().parent))

from eval import BASE, IMAGE_SIDE, reference_mesh
from pairs_data import DATA, PROMPT, VIEWS, read_pairs
from reward import score


def group_advantages(rewards, eps=1e-4):
    mean = sum(rewards) / len(rewards)
    std = (sum((r - mean) ** 2 for r in rewards) / len(rewards)) ** 0.5
    return [(r - mean) / (std + eps) for r in rewards]


def kl_k3(logp, ref_logp):
    delta = ref_logp - logp
    return torch.exp(delta) - delta - 1.0


def sequence_loss(adv, logp, ref_logp, beta):
    policy = -(adv * logp.mean())
    anchor = beta * kl_k3(logp, ref_logp).mean() if beta > 0 else 0.0
    return policy + anchor


def prompt_inputs(processor, row):
    images = [Image.open(DATA / row["renders"][v]).convert("RGB").resize((IMAGE_SIDE, IMAGE_SIDE)) for v in VIEWS]
    content = [{"type": "image"} for _ in images] + [{"type": "text", "text": PROMPT}]
    prompt = processor.apply_chat_template([{"role": "user", "content": content}], tokenize=False, add_generation_prompt=True)
    return processor(text=[prompt], images=images, return_tensors="pt")


def sequence_logprob(model, batch, completion_ids):
    input_ids = torch.cat([batch["input_ids"], completion_ids], dim=1)
    mask = torch.ones_like(input_ids)
    extra = {k: v for k, v in batch.items() if k not in ("input_ids", "attention_mask")}
    logits = model(input_ids=input_ids, attention_mask=mask, **extra).logits
    start = batch["input_ids"].shape[1]
    lp = torch.log_softmax(logits[:, start - 1 : -1].float(), dim=-1)
    return lp.gather(-1, completion_ids.unsqueeze(-1)).squeeze(-1)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adapter", required=True)
    parser.add_argument("--anchor", default="")
    parser.add_argument("--out", required=True)
    parser.add_argument("--base", default=BASE)
    parser.add_argument("--steps", type=int, default=1000)
    parser.add_argument("--group", type=int, default=8)
    parser.add_argument("--prompts", type=int, default=8)
    parser.add_argument("--beta", type=float, default=0.04)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--temperature", type=float, default=0.9)
    parser.add_argument("--max-new-tokens", type=int, default=600)
    parser.add_argument("--max-lines", type=int, default=40)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()

    random.seed(args.seed)
    processor = AutoProcessor.from_pretrained(args.base)
    model = AutoModelForImageTextToText.from_pretrained(args.base, dtype=torch.bfloat16, device_map={"": 0})
    model = PeftModel.from_pretrained(model, args.adapter, adapter_name="policy", is_trainable=True)
    model.load_adapter(args.anchor or args.adapter, adapter_name="anchor", is_trainable=False)
    model.set_adapter("policy")
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    optimizer = torch.optim.AdamW([p for p in model.parameters() if p.requires_grad], lr=args.lr)
    rows = read_pairs(split="train", max_lines=args.max_lines)
    pool = ThreadPoolExecutor(8)
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    log = open(out_dir.parent / (out_dir.name + "_log.jsonl"), "a")
    for step in range(args.steps):
        picked = random.sample(rows, args.prompts)
        samples = []
        rewards_all = []
        built_all = []
        for row in picked:
            batch = prompt_inputs(processor, row).to(model.device)
            model.eval()
            with torch.no_grad():
                out = model.generate(
                    **batch,
                    do_sample=True,
                    temperature=args.temperature,
                    max_new_tokens=args.max_new_tokens,
                    num_return_sequences=args.group,
                )
            completions = out[:, batch["input_ids"].shape[1] :]
            texts = processor.tokenizer.batch_decode(completions, skip_special_tokens=True)
            ref = reference_mesh(row)
            results = list(pool.map(lambda t: score(t, ref), texts))
            rewards = [r["reward"] for r in results]
            rewards_all.extend(rewards)
            built_all.extend(r["built"] for r in results)
            samples.append((batch, completions, group_advantages(rewards)))
        model.train()
        total = 0.0
        count = sum(1 for _, comps, advs in samples for a in advs if abs(a) >= 1e-6)
        for batch, completions, advantages in samples:
            for comp, adv in zip(completions, advantages):
                if abs(adv) < 1e-6:
                    continue
                ids = comp[comp != processor.tokenizer.pad_token_id].unsqueeze(0)
                if ids.shape[1] == 0:
                    continue
                with torch.no_grad():
                    model.set_adapter("anchor")
                    ref_lp = sequence_logprob(model, batch, ids)
                    model.set_adapter("policy")
                lp = sequence_logprob(model, batch, ids)
                loss = sequence_loss(adv, lp, ref_lp, args.beta) / max(count, 1)
                loss.backward()
                total += loss.item()
        torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        optimizer.zero_grad(set_to_none=True)
        record = {
            "step": step + 1,
            "ids": [r["id"] for r in picked],
            "mean_reward": sum(rewards_all) / len(rewards_all),
            "built": sum(built_all) / len(built_all),
            "loss": total,
        }
        log.write(json.dumps(record) + "\n")
        log.flush()
        if (step + 1) % 20 == 0:
            model.save_pretrained(args.out, selected_adapters=["policy"])
    model.save_pretrained(args.out, selected_adapters=["policy"])


if __name__ == "__main__":
    main()
