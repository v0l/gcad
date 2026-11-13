import argparse
import hashlib
import json
import random
import re
import sys
from collections import Counter
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from PIL import Image

DATA = Path("/home/kieran/gcad-datasets")
OUT = DATA / "out" / "pairs"
ZERO = json.loads((DATA / "out" / "zero-to-cad" / "manifest.jsonl").open().readline())


def split_of(uid):
    bucket = int(hashlib.sha1(uid.encode()).hexdigest(), 16) % 100
    return "val" if bucket < 5 else "test" if bucket < 10 else "train"


def normalise_reason(r):
    text = re.sub(r"line \d+: ", "", r.get("reason", ""))
    text = re.sub(r"[\d.]+", "N", text)
    return f"{r['status']}: {text[:90]}"


def percentile(values, q):
    values = sorted(values)
    return values[min(len(values) - 1, int(q * len(values)))]


def contact_sheet(kept, path, count=20, seed=3):
    chosen = random.Random(seed).sample(kept, min(count, len(kept)))
    cols = 4
    rows = (len(chosen) + cols - 1) // cols
    fig, axes = plt.subplots(rows, cols * 2, figsize=(cols * 4.4, rows * 2.4))
    for ax in axes.flat:
        ax.axis("off")
    for i, r in enumerate(chosen):
        uid = r["id"]
        source = Image.open(DATA / "out" / "zero-to-cad" / "obj" / uid[:2] / uid / "view0.png")
        mine = Image.open(DATA / r["renders"]["iso"])
        row, col = divmod(i, cols)
        axes[row, col * 2].imshow(source)
        axes[row, col * 2 + 1].imshow(mine)
        axes[row, col * 2].set_title("source", fontsize=6)
        axes[row, col * 2 + 1].set_title(f"gcad {r['lines']} lines", fontsize=6)
    fig.tight_layout()
    fig.savefig(path, dpi=110)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", default="results.jsonl")
    parser.add_argument("--sheet", default="")
    args = parser.parse_args()
    rows = [json.loads(l) for l in (OUT / args.results).open()]
    status = Counter(r["status"] for r in rows)
    total = len(rows)
    translated = sum(1 for r in rows if r.get("translated"))
    kept = [r for r in rows if r["status"] == "kept"]
    built_ok = sum(1 for r in rows if r.get("metrics"))
    reasons = Counter(normalise_reason(r) for r in rows if r["status"] != "kept")
    ops = Counter()
    for r in kept:
        ops.update(r["ops"])
    lines = [r["lines"] for r in kept]
    report = {
        "total": total,
        "translated": translated,
        "translate_rate": translated / total,
        "built": built_ok,
        "kept": len(kept),
        "keep_rate_of_translated": len(kept) / max(translated, 1),
        "keep_rate_of_all": len(kept) / total,
        "status": dict(status),
        "ops": dict(ops.most_common()),
        "lines": {"min": min(lines), "median": percentile(lines, 0.5), "p90": percentile(lines, 0.9), "max": max(lines)},
        "line_histogram": dict(sorted(Counter(min(l // 10 * 10, 100) for l in lines).items())),
        "top_drop_reasons": reasons.most_common(15),
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=1))
    print(json.dumps(report, indent=1))
    manifest = {}
    for line in (DATA / "out" / "zero-to-cad" / "manifest.jsonl").open():
        m = json.loads(line)
        manifest[m["id"].split(":", 1)[1]] = m
    with (OUT / "pairs.jsonl").open("w") as f:
        for r in sorted(kept, key=lambda r: r["id"]):
            m = manifest[r["id"]]
            f.write(json.dumps({
                "id": "zero-to-cad:" + r["id"],
                "source": "zero-to-cad",
                "license": m["license"],
                "commercial_ok": m["commercial_ok"],
                "gcad": r["gcad"],
                "renders": {**r["renders"], "right": r["renders"]["iso"].replace("_iso.png", "_right.png")},
                "source_render": m["paths"]["render"],
                "lines": r["lines"],
                "ops": r["ops"],
                "shape_distance": {
                    "volume_error": r["metrics"]["volume_error"],
                    "chamfer": r["metrics"]["chamfer"],
                    "hausdorff": r["metrics"]["hausdorff"],
                    "bbox_error": r["metrics"]["bbox_error"],
                },
                "split": split_of(r["id"]),
            }) + "\n")
    print("splits", Counter(split_of(r["id"]) for r in kept))
    if args.sheet:
        contact_sheet(kept, args.sheet)


if __name__ == "__main__":
    main()
