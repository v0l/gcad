import argparse
import json
import random
from pathlib import Path

from pairs_data import DATA, chat_example, op_histogram, read_pairs


def build(split, limit, seed, max_lines):
    rows = read_pairs(split=split, max_lines=max_lines)
    random.Random(seed).shuffle(rows)
    if limit:
        rows = rows[:limit]
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default=str(DATA / "out" / "vlm"))
    parser.add_argument("--train-limit", type=int, default=0)
    parser.add_argument("--max-lines", type=int, default=60)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--dims", action="store_true")
    parser.add_argument("--dims-p", type=float, default=0.3)
    parser.add_argument("--dims-seed", type=int, default=1)
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    for split in ("train", "val", "test"):
        rows = build(split, args.train_limit if split == "train" else 0, args.seed, args.max_lines)
        with open(out / f"{split}.jsonl", "w") as f:
            for row in rows:
                f.write(json.dumps(chat_example(row, dims=args.dims, p=args.dims_p, seed=args.dims_seed)) + "\n")
        print(split, len(rows), json.dumps(op_histogram(rows)))


if __name__ == "__main__":
    main()
