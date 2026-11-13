import argparse
import json
import statistics
from pathlib import Path

ANCHOR = "| SFT then GRPO, 120 steps |"


def summarise_file(path):
    results = [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]
    built = [r for r in results if r["built"]]
    good = [r for r in built if r["metrics"]["volume_error"] <= 0.01 and r["metrics"]["chamfer"] <= 0.005]
    n = max(1, len(results))
    return {
        "n": len(results),
        "build": len(built) / n,
        "match": len(good) / n,
        "reward": statistics.fmean(r["reward"] for r in results) if results else 0.0,
        "chamfer": statistics.median(r["metrics"]["chamfer"] for r in built) if built else None,
        "volume": statistics.median(r["metrics"]["volume_error"] for r in built) if built else None,
    }


def format_row(label, s):
    chamfer = "" if s["chamfer"] is None else f"{s['chamfer'] * 100:.2f}%"
    volume = "" if s["volume"] is None else f"{s['volume'] * 100:.1f}%"
    return f"| {label} | {s['build'] * 100:.0f}% | {s['match'] * 100:.0f}% | {s['reward']:.2f} | {chamfer} | {volume} |"


def insert_row(doc_text, row):
    lines = doc_text.splitlines()
    label = row.split("|")[1].strip()
    lines = [line for line in lines if not line.startswith(f"| {label} |")]
    index = next(i for i, line in enumerate(lines) if line.startswith(ANCHOR))
    lines.insert(index + 1, row)
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", required=True)
    parser.add_argument("--doc", required=True)
    parser.add_argument("--label", required=True)
    args = parser.parse_args()
    row = format_row(args.label, summarise_file(args.results))
    doc = Path(args.doc)
    doc.write_text(insert_row(doc.read_text(), row))
    print(row)


if __name__ == "__main__":
    main()
