import argparse
import json
import os

DATA = os.path.expanduser("~/gcad-datasets")


def rel(path):
    p = os.path.abspath(path)
    return os.path.relpath(p, DATA) if p.startswith(DATA) else path


def mark_real(record):
    if record.get("has_concept"):
        record["concept_kind"] = "real"
    return record


def mark_synthetic(record, gen):
    record["has_concept"] = True
    record["concept_kind"] = "synthetic"
    record.setdefault("paths", {})["concept"] = [rel(gen["concept"])]
    record["generator"] = gen["generator"]
    record["prompt"] = gen["prompt"]
    record["concept_silhouette_iou"] = gen["silhouette_iou"]
    return record


def merge(manifest, kept, out):
    by_id = {}
    for line in open(kept):
        g = json.loads(line)
        by_id[g["id"]] = g
    n_syn = n_real = 0
    with open(manifest) as src, open(out, "w") as dst:
        for line in src:
            m = json.loads(line)
            if m["id"] in by_id and not m.get("has_concept"):
                mark_synthetic(m, by_id[m["id"]])
                n_syn += 1
            else:
                mark_real(m)
                n_real += m.get("concept_kind") == "real"
            dst.write(json.dumps(m) + "\n")
    return n_syn, n_real


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default=os.path.join(DATA, "out/manifest.jsonl"))
    ap.add_argument("--kept", default=os.path.join(DATA, "out/synthetic/kept.jsonl"))
    ap.add_argument("--out", default=os.path.join(DATA, "out/manifest.synthetic.jsonl"))
    args = ap.parse_args()
    syn, real = merge(args.manifest, args.kept, args.out)
    print(syn, "synthetic", real, "real")


if __name__ == "__main__":
    main()
