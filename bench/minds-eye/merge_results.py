import argparse
import json
from pathlib import Path

import harness


def merge(parts, tasks):
    rows = {}
    for part in parts:
        for r in part["rows"]:
            if r["task"] in tasks:
                rows[(r["task"], r["seed"])] = r
    ordered = [rows[k] for k in sorted(rows, key=lambda k: (tasks.index(k[0]), k[1]))]
    out = dict(parts[-1])
    out["rows"] = ordered
    out["summary"] = harness.summarize(ordered)
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("parts", nargs="+")
    ap.add_argument("--tasks", nargs="+", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    merged = merge([json.loads(Path(p).read_text()) for p in args.parts], args.tasks)
    Path(args.out).write_text(json.dumps(merged, indent=1))
    print(len(merged["rows"]), "rows", json.dumps(merged["summary"]))


if __name__ == "__main__":
    main()
