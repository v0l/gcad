import argparse
import json
import random
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import overlap
from expert_iter import example, key, normalise, read_jsonl, target_program

CHOSEN_MIN = 0.5
PAIRS_PER_PROMPT = 2


def prompt_pairs(entry, gt, chosen_min=CHOSEN_MIN, per_prompt=PAIRS_PER_PROMPT):
    built = sorted((s for s in entry["samples"] if s["built"] and s["stopped"]), key=lambda s: -s["reward"])
    failed = []
    for s in sorted(entry["samples"], key=lambda s: not s["stopped"]):
        program = normalise(s["program"])
        if not s["built"] and program not in failed:
            failed.append(program)
    if not failed:
        return []
    chosen = normalise(built[0]["program"]) if built and built[0]["reward"] >= chosen_min else normalise(gt)
    return [(chosen, rejected) for rejected in failed[:per_prompt] if rejected != chosen]


def build_pairs(sampled, rows_by_key, chosen_min=CHOSEN_MIN, per_prompt=PAIRS_PER_PROMPT):
    out = []
    for entry in sampled:
        row = rows_by_key[key(entry)]
        for chosen, rejected in prompt_pairs(entry, target_program(row), chosen_min, per_prompt):
            out.append({"chosen": example(row, chosen), "rejected": example(row, rejected)})
    return out


def preference_loss(policy_chosen, policy_rejected, ref_chosen, ref_rejected, chosen_tokens, beta, alpha):
    import torch.nn.functional as F

    margin = (policy_chosen - ref_chosen) - (policy_rejected - ref_rejected)
    dpo = -F.logsigmoid(beta * margin)
    nll = -policy_chosen / chosen_tokens.clamp(min=1)
    return dpo + alpha * nll, margin


def sequence_logps(model, batch):
    import torch

    labels = batch.pop("labels")
    logits = model(**batch).logits[:, :-1]
    targets = labels[:, 1:]
    mask = targets != -100
    picked = logits.gather(-1, targets.clamp(min=0).unsqueeze(-1)).squeeze(-1).float()
    lp = picked - torch.logsumexp(logits.float(), dim=-1)
    return (lp * mask).sum(-1), mask.sum(-1)


def train(args):
    import torch
    from peft import PeftModel
    from transformers import AutoModelForImageTextToText, AutoProcessor

    from prompt_format import BASE, format_for
    from sft import to_batch

    overlap.refuse_minds_eye(args.data)
    for path in args.samples:
        overlap.refuse_minds_eye(path)
    rows_by_key = {key(r): r for r in read_jsonl(args.data)}
    pairs = build_pairs([e for p in args.samples for e in read_jsonl(p)], rows_by_key, args.chosen_min)
    rng = random.Random(args.seed)
    rng.shuffle(pairs)
    torch.manual_seed(args.seed)
    print(json.dumps({"pairs": len(pairs)}), flush=True)

    fmt = format_for(BASE)
    processor = AutoProcessor.from_pretrained(BASE)
    processor.tokenizer.padding_side = "right"
    model = AutoModelForImageTextToText.from_pretrained(BASE, dtype=torch.bfloat16, device_map={"": 0})
    model = PeftModel.from_pretrained(model, args.adapter, adapter_name="policy", is_trainable=True)
    model.load_adapter(args.adapter, adapter_name="ref", is_trainable=False)
    model.set_adapter("policy")
    model.gradient_checkpointing_enable()
    model.enable_input_require_grads()
    params = [p for n, p in model.named_parameters() if p.requires_grad and ".policy." in n]
    optimizer = torch.optim.AdamW(params, lr=args.lr, weight_decay=0.0)
    steps = args.steps or max(1, len(pairs) * args.epochs // args.accum)
    schedule = torch.optim.lr_scheduler.LambdaLR(optimizer, lambda s: min(1.0, (s + 1) / 10) * max(0.1, 1 - s / steps))
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    log = open(out.parent / f"{out.name}_log.jsonl", "a")
    started = time.time()
    index = 0
    for step in range(steps):
        model.train()
        total, margins, wins = 0.0, [], 0
        for _ in range(args.accum):
            pair = pairs[index % len(pairs)]
            index += 1
            batch = to_batch(processor, [pair["chosen"], pair["rejected"]], model.device, fmt)
            with torch.no_grad():
                model.set_adapter("ref")
                ref, _ = sequence_logps(model, dict(batch))
                model.set_adapter("policy")
            policy, tokens = sequence_logps(model, dict(batch))
            loss, margin = preference_loss(policy[0], policy[1], ref[0], ref[1], tokens[0], args.beta, args.alpha)
            (loss / args.accum).backward()
            total += loss.item() / args.accum
            margins.append(margin.item())
            wins += margin.item() > 0
        torch.nn.utils.clip_grad_norm_(params, 1.0)
        optimizer.step()
        schedule.step()
        optimizer.zero_grad(set_to_none=True)
        record = {"step": step + 1, "loss": total, "margin": sum(margins) / len(margins), "win": wins / args.accum, "seconds": round(time.time() - started, 1)}
        log.write(json.dumps(record) + "\n")
        log.flush()
        if (step + 1) % 10 == 0:
            print(record, flush=True)
        if (step + 1) % args.save_every == 0 or step + 1 == steps:
            model.save_pretrained(args.out, selected_adapters=["policy"])
    processor.save_pretrained(args.out)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adapter", required=True)
    parser.add_argument("--data", required=True)
    parser.add_argument("--samples", nargs="+", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--beta", type=float, default=0.1)
    parser.add_argument("--alpha", type=float, default=0.5)
    parser.add_argument("--lr", type=float, default=2e-5)
    parser.add_argument("--accum", type=int, default=8)
    parser.add_argument("--epochs", type=int, default=1)
    parser.add_argument("--steps", type=int, default=0)
    parser.add_argument("--chosen-min", type=float, default=CHOSEN_MIN)
    parser.add_argument("--save-every", type=int, default=100)
    parser.add_argument("--seed", type=int, default=1)
    train(parser.parse_args())


if __name__ == "__main__":
    main()
