import argparse
import json
import sys
from pathlib import Path

HARD_MIN = 6.0
JUDGE_MIN = 5.0


def keep_row(row, hard_min=HARD_MIN, judge_min=JUDGE_MIN):
    hard = row.get("hard") or {}
    if not hard.get("builds"):
        return False
    if (hard.get("hard_score") or 0) < hard_min:
        return False
    judged = row.get("judge") or {}
    return float(judged.get("overall") or 0) >= judge_min


def read_transcript(agent_log):
    for line in Path(agent_log).read_text().splitlines():
        record = json.loads(line)
        if "transcript" in record:
            return record["transcript"]
    return None


def to_turn(message):
    role = message["role"]
    content = message.get("content")
    if isinstance(content, str):
        content = [{"type": "text", "text": content}]
    turn = {"role": role, "content": content or []}
    if message.get("tool_calls"):
        turn["tool_calls"] = [
            {"id": c["id"], "name": c["function"]["name"], "arguments": c["function"]["arguments"]}
            for c in message["tool_calls"]
        ]
    if message.get("tool_call_id"):
        turn["tool_call_id"] = message["tool_call_id"]
    return turn


def trajectory_example(task_id, transcript):
    turns = [to_turn(m) for m in transcript if m["role"] != "system"]
    system = next((m["content"] for m in transcript if m["role"] == "system"), "")
    return {"id": task_id, "system": system, "messages": turns}


def collect(results_json, runs_dir, hard_min=HARD_MIN, judge_min=JUDGE_MIN):
    result = json.loads(Path(results_json).read_text())
    out = []
    for row in result["rows"]:
        if not keep_row(row, hard_min, judge_min):
            continue
        log = Path(runs_dir) / f"{row['task']}-s{row['seed']}" / "agent.jsonl"
        if not log.exists():
            continue
        transcript = read_transcript(log)
        if transcript:
            out.append(trajectory_example(f"{row['task']}-s{row['seed']}", transcript))
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("results")
    parser.add_argument("runs_dir")
    parser.add_argument("--out", required=True)
    parser.add_argument("--hard-min", type=float, default=HARD_MIN)
    parser.add_argument("--judge-min", type=float, default=JUDGE_MIN)
    args = parser.parse_args()
    rows = collect(args.results, args.runs_dir, args.hard_min, args.judge_min)
    with open(args.out, "a") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    print(len(rows), "trajectories kept", file=sys.stderr)


if __name__ == "__main__":
    main()
