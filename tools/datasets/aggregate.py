import json
import sys
from collections import Counter, defaultdict
from common import *

SOURCES = ["blender-studio", "zero-to-cad", "deepcad", "cad-recode", "fusion360", "kenney", "polyhaven",
           "objaverse", "abo", "gso", "pix3d", "thingi10k"]
FIELDS = ["id", "source", "url", "license", "commercial_ok", "attribution", "category", "paths", "has_concept",
          "has_build_history", "has_photo", "triangles", "watertight", "gcad_fit", "gcad_fit_score", "shape_kind"]


def merge():
    out = DATA / "out"
    allrows, alldrops, stats = [], [], {}
    for s in SOURCES:
        m = out / s / "manifest.jsonl"
        if not m.exists():
            continue
        rows = read_jsonl(m)
        drops = read_jsonl(out / s / "dropped.jsonl") if (out / s / "dropped.jsonl").exists() else []
        for r in rows:
            for k in FIELDS:
                r.setdefault(k, None)
        allrows += rows
        alldrops += drops
        stats[s] = {"rows": rows, "dropped": len(drops)}
    write_jsonl(out / "manifest.jsonl", allrows)
    write_jsonl(out / "dropped.jsonl", alldrops)
    reasons = Counter()
    for d in alldrops:
        reasons[(d["source"], d["reason"].split(":")[0])] += 1
    (out / "drop-reasons.json").write_text(json.dumps({f"{k[0]}|{k[1]}": v for k, v in reasons.items()}, indent=1))
    summary = {}
    for s, v in stats.items():
        rows = v["rows"]
        summary[s] = {
            "count": len(rows), "dropped": v["dropped"], "license": sorted({r["license"] for r in rows}),
            "commercial_ok": sum(1 for r in rows if r["commercial_ok"]),
            "concept": sum(1 for r in rows if r["has_concept"]),
            "build_history": sum(1 for r in rows if r["has_build_history"]),
            "photo": sum(1 for r in rows if r["has_photo"]),
            "mesh": sum(1 for r in rows if r["paths"].get("glb")),
            "fit": dict(Counter(r.get("gcad_fit") for r in rows)),
            "categories": dict(Counter(r["category"] for r in rows).most_common(8)),
            "disk_gb": round(du_gb(DATA / "raw" / s) + du_gb(DATA / "out" / s), 1),
        }
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    print(len(allrows), "rows,", len(alldrops), "dropped")
    for s, v in summary.items():
        print(s, v["count"], v["dropped"], v["disk_gb"])


if __name__ == "__main__":
    merge()
