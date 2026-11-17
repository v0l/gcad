import argparse
import json
import random
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import overlap
from pairs_data import read_pairs
from reward import strip_fences

REWARD_MIN = 0.75


def read_jsonl(path):
    return [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]


def is_image_row(row):
    return any(c["type"] == "image" for c in row["messages"][0]["content"])


def kind(row):
    return "image" if is_image_row(row) else "brief"


def key(row):
    return (row["id"], row.get("kind") or kind(row))


def prompt_parts(row):
    content = row["messages"][0]["content"]
    images = [c["image"] for c in content if c["type"] == "image"]
    text = " ".join(c["text"] for c in content if c["type"] == "text")
    return images, text


def target_program(row):
    return row["messages"][1]["content"][0]["text"]


def pick_prompts(rows, n_briefs, n_images, seed, exclude=(), allowed=None):
    skip = set(exclude)
    usable = [r for r in rows if key(r) not in skip and (allowed is None or r["id"] in allowed)]
    images = [r for r in usable if is_image_row(r)]
    briefs = [r for r in usable if not is_image_row(r)]
    rng = random.Random(seed)
    picked = rng.sample(briefs, min(n_briefs, len(briefs))) + rng.sample(images, min(n_images, len(images)))
    rng.shuffle(picked)
    return picked


def normalise(program):
    return strip_fences(program).strip() + "\n"


def choose_targets(samples, gt, reward_min=REWARD_MIN):
    best = {}
    for s in samples:
        if not (s["built"] and s["stopped"] and s["reward"] >= reward_min):
            continue
        program = normalise(s["program"])
        if program not in best or s["reward"] > best[program]:
            best[program] = s["reward"]
    good = sorted(best, key=lambda p: -best[p])
    wanted = 2 if any(not s["built"] for s in samples) else 1
    chosen = good[:wanted]
    if len(chosen) < wanted and normalise(gt) not in chosen:
        chosen.append(normalise(gt))
    return chosen


def example(row, program):
    return {
        "id": row["id"],
        "messages": [row["messages"][0], {"role": "assistant", "content": [{"type": "text", "text": program}]}],
    }


def build_rows(sampled, rows_by_key, replay_rows, reward_min=REWARD_MIN):
    out = []
    stats = {"prompts": 0, "with_failures": 0, "own_targets": 0, "gt_targets": 0, "replay": len(replay_rows)}
    for entry in sampled:
        row = rows_by_key[key(entry)]
        gt = normalise(target_program(row))
        chosen = choose_targets(entry["samples"], gt, reward_min)
        stats["prompts"] += 1
        stats["with_failures"] += any(not s["built"] for s in entry["samples"])
        for program in chosen:
            stats["gt_targets" if program == gt else "own_targets"] += 1
            out.append(example(row, program))
    out.extend(example(r, normalise(target_program(r))) for r in replay_rows)
    return out, stats


def done_ids(path):
    if not Path(path).exists():
        return set()
    return {key(j) for j in read_jsonl(path)}


def train_ids():
    return {r["id"] for r in read_pairs(split="train", commercial_only=False)}


def sample(args):
    import torch
    from peft import PeftModel
    from PIL import Image
    from transformers import AutoModelForImageTextToText, AutoProcessor

    from eval import IMAGE_SIDE, reference_mesh
    from prompt_format import BASE, completion_tokens, format_for, prompt_text, stop_ids
    from reward import score

    overlap.refuse_minds_eye(args.data)
    rows = read_jsonl(args.data)
    exclude = set()
    for path in args.exclude:
        exclude |= done_ids(path)
    picked = pick_prompts(rows, args.briefs, args.images, args.seed, exclude, train_ids())
    finished = done_ids(args.out)
    todo = [r for r in picked if key(r) not in finished]
    pairs = {p["id"]: p for p in read_pairs(split="train", commercial_only=False)}
    fmt = format_for(BASE)
    processor = AutoProcessor.from_pretrained(BASE)
    stop = stop_ids(processor.tokenizer, fmt)[0]
    pad = processor.tokenizer.pad_token_id
    model = AutoModelForImageTextToText.from_pretrained(BASE, dtype=torch.bfloat16, device_map={"": 0})
    model = PeftModel.from_pretrained(model, args.adapter)
    model.eval()
    print(json.dumps({"picked": len(picked), "todo": len(todo)}), flush=True)

    def scored(row, completions):
        ref = reference_mesh(pairs[row["id"]])
        samples = []
        for ids in completions:
            text = processor.tokenizer.decode(ids, skip_special_tokens=True)
            r = score(text, ref)
            samples.append({"program": text, "built": r["built"], "reward": r["reward"], "error": r["error"], "stopped": bool(ids) and ids[-1] == stop})
        return {"id": row["id"], "kind": kind(row), "samples": samples}

    with ThreadPoolExecutor(args.workers) as pool, open(args.out, "a") as f:
        pending = []
        for n, row in enumerate(todo, 1):
            images, text = prompt_parts(row)
            loaded = [Image.open(p).convert("RGB").resize((IMAGE_SIDE, IMAGE_SIDE)) for p in images]
            batch = processor(text=[prompt_text(processor, fmt, len(loaded), text)], images=loaded or None, return_tensors="pt").to(model.device)
            with torch.no_grad():
                out = model.generate(**batch, do_sample=True, temperature=args.temperature, max_new_tokens=args.max_new_tokens,
                                     num_return_sequences=args.k, eos_token_id=[stop], pad_token_id=pad if pad is not None else stop)
            completions = [completion_tokens(c.tolist(), stop, pad) for c in out[:, batch["input_ids"].shape[1]:]]
            pending.append(pool.submit(scored, row, completions))
            while pending and pending[0].done():
                f.write(json.dumps(pending.pop(0).result()) + "\n")
                f.flush()
            if n % 50 == 0:
                print(json.dumps({"sampled": n, "of": len(todo)}), flush=True)
        for future in pending:
            f.write(json.dumps(future.result()) + "\n")


def build(args):
    overlap.refuse_minds_eye(args.data)
    rows = read_jsonl(args.data)
    rows_by_key = {key(r): r for r in rows}
    sampled = [e for path in args.samples for e in read_jsonl(path)]
    seen = {key(e) for e in sampled}
    allowed = train_ids()
    rng = random.Random(args.seed)
    rest = [r for r in rows if key(r) not in seen and r["id"] in allowed]
    replay = rng.sample(rest, min(args.replay, len(rest)))
    out, stats = build_rows(sampled, rows_by_key, replay, args.reward_min)
    rng.shuffle(out)
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text("".join(json.dumps(r) + "\n" for r in out))
    stats["rows"] = len(out)
    print(json.dumps(stats))


def summary(args):
    entries = [e for path in args.samples for e in read_jsonl(path)]
    for kind in ("brief", "image"):
        group = [e for e in entries if e["kind"] == kind]
        samples = [s for e in group for s in e["samples"]]
        if not samples:
            continue
        print(json.dumps({
            "kind": kind,
            "prompts": len(group),
            "sample_build": sum(s["built"] for s in samples) / len(samples),
            "prompts_all_built": sum(all(s["built"] for s in e["samples"]) for e in group) / len(group),
            "prompts_none_built": sum(not any(s["built"] for s in e["samples"]) for e in group) / len(group),
            "sample_reward": sum(s["reward"] for s in samples) / len(samples),
        }))


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    s = sub.add_parser("sample")
    s.add_argument("--adapter", required=True)
    s.add_argument("--data", required=True)
    s.add_argument("--out", required=True)
    s.add_argument("--briefs", type=int, default=1100)
    s.add_argument("--images", type=int, default=500)
    s.add_argument("--k", type=int, default=8)
    s.add_argument("--temperature", type=float, default=0.8)
    s.add_argument("--max-new-tokens", type=int, default=700)
    s.add_argument("--seed", type=int, default=1)
    s.add_argument("--workers", type=int, default=4)
    s.add_argument("--exclude", nargs="*", default=[])
    b = sub.add_parser("build")
    b.add_argument("--data", required=True)
    b.add_argument("--samples", nargs="+", required=True)
    b.add_argument("--out", required=True)
    b.add_argument("--replay", type=int, default=2000)
    b.add_argument("--reward-min", type=float, default=REWARD_MIN)
    b.add_argument("--seed", type=int, default=1)
    m = sub.add_parser("summary")
    m.add_argument("--samples", nargs="+", required=True)
    args = parser.parse_args()
    {"sample": sample, "build": build, "summary": summary}[args.command](args)


if __name__ == "__main__":
    main()
