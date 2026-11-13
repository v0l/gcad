import argparse
import json
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "tools", "vlm"))

from pairs_data import read_pairs

CATEGORY = "mechanical part"


def stratified(rows, count, seed):
    groups = {}
    for row in rows:
        groups.setdefault(tuple(sorted(row["ops"])), []).append(row)
    rng = random.Random(seed)
    for key in groups:
        groups[key].sort(key=lambda r: r["id"])
        rng.shuffle(groups[key])
    total = len(rows)
    quotas = {k: len(v) * count / total for k, v in groups.items()}
    taken = {k: int(q) for k, q in quotas.items()}
    leftover = count - sum(taken.values())
    for k in sorted(quotas, key=lambda k: (quotas[k] - taken[k], k), reverse=True)[:leftover]:
        taken[k] += 1
    chosen = []
    for k, v in groups.items():
        chosen.extend(v[: taken[k]])
    return chosen


def selection_record(row):
    uuid = row["id"].split(":")[1]
    return {
        "id": row["id"],
        "source": row["source"],
        "split": row["split"],
        "category": CATEGORY,
        "commercial_ok": row["commercial_ok"],
        "paths": {"glb": f"out/zero-to-cad/glb/{uuid[:2]}/{uuid}.glb"},
    }


def select(train, val, test, train_count, seed):
    chosen = stratified(train, min(train_count, len(train)), seed) + val + test
    chosen.sort(key=lambda r: r["id"])
    return [selection_record(r) for r in chosen]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--train-count", type=int, default=2000)
    ap.add_argument("--max-lines", type=int, default=60)
    ap.add_argument("--seed", type=int, default=563)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    train = read_pairs(split="train", max_lines=args.max_lines)
    val = read_pairs(split="val", max_lines=args.max_lines)
    test = read_pairs(split="test", max_lines=args.max_lines)
    records = select(train, val, test, args.train_count, args.seed)
    with open(args.out, "w") as f:
        for r in records:
            f.write(json.dumps(r) + "\n")
    print(len(records), "selected")


if __name__ == "__main__":
    main()
