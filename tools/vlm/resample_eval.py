import argparse
import json
import statistics
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from eval import reference_mesh
from pairs_data import read_pairs
from repair import drop_failing_lines
from reward import score, strip_fences


def first_built(sample_row):
    for index, error in enumerate(sample_row["errors"]):
        if error is None:
            return index
    return None


def loop_row(row, samples, pair):
    if row["built"]:
        return dict(row, tries=1, stage="single_shot", single_shot=True)
    entry = samples.get(row["id"])
    index = first_built(entry) if entry else None
    if index is not None:
        program = strip_fences(entry["programs"][index])
        tries, stage = index + 2, "resample"
    else:
        program, _, _ = drop_failing_lines(strip_fences(row["program"]))
        tries, stage = 1 + (entry["k"] if entry else 0) + 1, "drop_lines"
    result = score(program, reference_mesh(pair))
    result.update({"id": row["id"], "program": program, "tries": tries, "stage": stage, "single_shot": False})
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
    parser.add_argument("--samples", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    rows = [json.loads(l) for l in Path(args.results).read_text().splitlines() if l.strip()]
    samples = {j["id"]: j for j in map(json.loads, Path(args.samples).read_text().splitlines()) if j}
    pairs = {p["id"]: p for p in read_pairs(split="test")}
    with ThreadPoolExecutor(1) as pool:
        results = list(pool.map(lambda r: loop_row(r, samples, pairs[r["id"]]), rows))
    Path(args.out).write_text("".join(json.dumps(r) + "\n" for r in results))
    print(json.dumps(summarise(results)))


if __name__ == "__main__":
    main()
