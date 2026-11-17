import argparse
import json
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from pairs_data import DATA, PAIRS
from repair import build_error


def check(row):
    program = (DATA / row["gcad"]).read_text()
    return row["id"], build_error(program, timeout=60)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--split", default="train")
    parser.add_argument("--workers", type=int, default=3)
    args = parser.parse_args()
    rows = [json.loads(l) for l in PAIRS.read_text().splitlines() if l.strip()]
    rows = [r for r in rows if r["split"] == args.split]
    failed = 0
    with ThreadPoolExecutor(args.workers) as pool, open(args.out, "w") as f:
        for ident, error in pool.map(check, rows):
            if error:
                failed += 1
                f.write(json.dumps({"id": ident, "error": error[:200]}) + "\n")
    print(json.dumps({"checked": len(rows), "failed": failed}))


if __name__ == "__main__":
    main()
