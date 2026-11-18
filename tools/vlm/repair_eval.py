import argparse
import json
import statistics
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from eval import reference_mesh
from repair import repair
from reward import score


def repaired_row(row, k=1):
    outcome = repair(row["program"], lambda: "", k)
    result = score(outcome["program"], reference_mesh(row))
    result.update({"id": row["id"], "program": outcome["program"], "stage": outcome["stage"], "tries": outcome["tries"], "single_shot": row["built"]})
    return result


def summarise(results):
    built = [r for r in results if r["built"]]
    good = [r for r in built if r["metrics"]["volume_error"] <= 0.01 and r["metrics"]["chamfer"] <= 0.005]
    n = max(1, len(results))
    stages = {}
    for r in results:
        stages[r["stage"]] = stages.get(r["stage"], 0) + 1
    return {
        "n": len(results),
        "single_shot_build": sum(r["single_shot"] for r in results) / n,
        "loop_build": len(built) / n,
        "match": len(good) / n,
        "mean_reward": statistics.fmean(r["reward"] for r in results) if results else 0.0,
        "mean_tries": statistics.fmean(r["tries"] for r in results) if results else 0.0,
        "stages": stages,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("results")
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    rows = [json.loads(l) for l in Path(args.results).read_text().splitlines() if l.strip()]
    with ThreadPoolExecutor(1) as pool:
        results = list(pool.map(repaired_row, rows))
    Path(args.out).write_text("".join(json.dumps(r) + "\n" for r in results))
    print(json.dumps(summarise(results)))


if __name__ == "__main__":
    main()
