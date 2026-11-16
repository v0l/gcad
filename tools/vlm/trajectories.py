import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import overlap

REWARD_MIN = 0.5


def keep_run(run, reward_min=REWARD_MIN):
    return bool(run.get("built")) and float(run.get("reward") or 0) >= reward_min


def to_turn(message):
    content = message.get("content")
    if isinstance(content, str):
        content = [{"type": "text", "text": content}]
    turn = {"role": message["role"], "content": content or []}
    if message.get("tool_calls"):
        turn["tool_calls"] = [
            {"id": c["id"], "name": c["function"]["name"], "arguments": c["function"]["arguments"]}
            for c in message["tool_calls"]
        ]
    if message.get("tool_call_id"):
        turn["tool_call_id"] = message["tool_call_id"]
    return turn


def trajectory_example(run_id, transcript):
    system = next((m["content"] for m in transcript if m["role"] == "system"), "")
    return {"id": run_id, "system": system, "messages": [to_turn(m) for m in transcript if m["role"] != "system"]}


def images_of(example):
    return [p["image"] for m in example["messages"] for p in m["content"] if isinstance(p, dict) and p.get("type") == "image"]


def clean_example(example):
    return not any(overlap.inside_minds_eye(p) for p in images_of(example) if p)


def collect(runs_path, banned, reward_min=REWARD_MIN):
    overlap.refuse_minds_eye(runs_path)
    out = []
    for line in Path(runs_path).read_text().splitlines():
        if not line.strip():
            continue
        run = json.loads(line)
        if not keep_run(run, reward_min):
            continue
        if overlap.overlaps_minds_eye(run["brief"], banned):
            continue
        example = trajectory_example(run["id"], run["transcript"])
        if clean_example(example):
            out.append(example)
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("runs", help="jsonl of {id, brief, built, reward, transcript} from general requests")
    parser.add_argument("--out", required=True)
    parser.add_argument("--reward-min", type=float, default=REWARD_MIN)
    args = parser.parse_args()
    rows = collect(args.runs, overlap.load_minds_eye(), args.reward_min)
    with open(args.out, "a") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    print(len(rows), "trajectories kept", file=sys.stderr)


if __name__ == "__main__":
    main()
