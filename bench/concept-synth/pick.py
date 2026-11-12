import argparse
import json
import os
import random

DATA = os.path.expanduser("~/gcad-datasets")


def load(manifest):
    with open(manifest) as f:
        for line in f:
            yield json.loads(line)


def eligible(m, fits):
    if m.get("has_concept"):
        return False
    if not m.get("commercial_ok"):
        return False
    if m.get("gcad_fit") not in fits:
        return False
    if m.get("triangles", 0) < 50 or m.get("triangles", 0) > 400000:
        return False
    return bool(m.get("paths", {}).get("glb"))


def select(records, per_source, seed, fits=("high", "medium")):
    by_source = {}
    for m in records:
        if eligible(m, fits):
            by_source.setdefault(m["source"], []).append(m)
    rng = random.Random(seed)
    chosen = []
    for source in sorted(by_source):
        pool = sorted(by_source[source], key=lambda m: m["id"])
        rng.shuffle(pool)
        chosen.extend(pool[:per_source])
    return chosen


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default=os.path.join(DATA, "out/manifest.jsonl"))
    ap.add_argument("--per-source", type=int, default=40)
    ap.add_argument("--seed", type=int, default=553)
    ap.add_argument("--out", default="selection.jsonl")
    args = ap.parse_args()
    chosen = select(load(args.manifest), args.per_source, args.seed)
    with open(args.out, "w") as f:
        for m in chosen:
            f.write(json.dumps(m) + "\n")
    print(len(chosen), "selected")


if __name__ == "__main__":
    main()
