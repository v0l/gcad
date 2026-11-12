import argparse
import json
import os

import numpy as np
from PIL import Image

import filter_iou

DATA = os.path.expanduser("~/gcad-datasets")
BG = (236, 236, 232)
GRID = 24


def target(record):
    lo = np.array(record["bbox_min"], dtype=np.float64)
    hi = np.array(record["bbox_max"], dtype=np.float64)
    ext = np.maximum(hi - lo, 1e-6)
    ratios = np.sort(ext / ext.max())
    return np.concatenate([np.log(ratios[:2]), [np.log1p(record.get("parts", 1))]])


def features(path):
    rgba = Image.open(path).convert("RGBA")
    if np.asarray(rgba)[:, :, 3].min() < 255:
        bg = Image.new("RGBA", rgba.size, BG + (255,))
        bg.alpha_composite(rgba)
        rgba = bg
    rgb = rgba.convert("RGB")
    grey = np.asarray(rgb.convert("L").resize((GRID, GRID), Image.BILINEAR), dtype=np.float32) / 255.0
    mask = filter_iou.concept_mask(path if rgba.getextrema()[3][0] == 255 else _flat(path)).astype(np.float32)
    mask = np.asarray(Image.fromarray(mask * 255).resize((GRID, GRID), Image.BILINEAR), dtype=np.float32) / 255.0
    ys, xs = np.nonzero(mask > 0.5)
    if len(xs):
        box = [(xs.max() - xs.min() + 1) / GRID, (ys.max() - ys.min() + 1) / GRID, mask.mean()]
    else:
        box = [0.0, 0.0, 0.0]
    return np.concatenate([mask.ravel(), (grey * mask).ravel() * 0.25, np.array(box) * 4.0, [1.0]])


def _flat(path):
    import tempfile
    rgba = Image.open(path).convert("RGBA")
    bg = Image.new("RGBA", rgba.size, BG + (255,))
    bg.alpha_composite(rgba)
    f = tempfile.NamedTemporaryFile(suffix=".png", delete=False)
    bg.convert("RGB").save(f.name)
    return f.name


def train(x, y, steps, batch, seed, lr=0.05, l2=1e-3):
    rng = np.random.default_rng(seed)
    w = np.zeros((x.shape[1], y.shape[1]))
    for _ in range(steps):
        idx = rng.integers(0, len(x), batch)
        xb, yb = x[idx], y[idx]
        grad = xb.T @ (xb @ w - yb) / batch + l2 * w
        w -= lr * grad
    return w


def error(w, x, y):
    return float(np.abs(x @ w - y).mean())


def arm_data(arm, synth, real_train):
    xs, ys = [], []
    if arm in ("A", "B"):
        for s in synth:
            xs.append(features(s["render"]))
            ys.append(target(s["record"]))
    if arm == "B":
        for s in synth:
            xs.append(features(s["concept"]))
            ys.append(target(s["record"]))
    if arm == "C":
        for r in real_train:
            xs.append(features(r["image"]))
            ys.append(target(r["record"]))
    return np.array(xs), np.array(ys)


def run(synth, real, steps=2000, batch=16, seeds=5):
    results = {a: [] for a in "ABC"}
    objects = sorted({r["record"]["id"] for r in real})
    for held in objects:
        test = [r for r in real if r["record"]["id"] == held]
        rest = [r for r in real if r["record"]["id"] != held]
        xt = np.array([features(r["image"]) for r in test])
        yt = np.array([target(r["record"]) for r in test])
        for arm in "ABC":
            x, y = arm_data(arm, synth, rest)
            errs = []
            for seed in range(seeds):
                w = train(x, y, steps, batch, seed)
                errs.append(error(w, xt, yt))
            results[arm].append({"held_out": held, "mae": float(np.mean(errs)), "train_pairs": len(x)})
    return results


def summary(results):
    return {a: float(np.mean([r["mae"] for r in rs])) for a, rs in results.items()}


def load_synth(kept, manifest):
    recs = {}
    for line in open(manifest):
        m = json.loads(line)
        recs[m["id"]] = m
    out = []
    for line in open(kept):
        g = json.loads(line)
        out.append({"concept": g["concept"], "render": g["render"], "record": recs[g["id"]]})
    return out


def load_real(manifest):
    out = []
    for line in open(manifest):
        m = json.loads(line)
        if m.get("has_concept") and m.get("concept_kind", "real") == "real":
            for p in m["paths"]["concept"]:
                out.append({"image": os.path.join(DATA, p), "record": m})
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default=os.path.join(DATA, "out/manifest.jsonl"))
    ap.add_argument("--kept", default=os.path.join(DATA, "out/synthetic/kept.jsonl"))
    ap.add_argument("--steps", type=int, default=2000)
    ap.add_argument("--batch", type=int, default=16)
    ap.add_argument("--seeds", type=int, default=5)
    ap.add_argument("--out", default="ablation.json")
    args = ap.parse_args()
    synth = load_synth(args.kept, args.manifest)
    real = load_real(args.manifest)
    res = run(synth, real, args.steps, args.batch, args.seeds)
    out = {"summary_mae": summary(res), "per_object": res, "synthetic_objects": len(synth),
           "real_pairs": len(real), "steps": args.steps, "batch": args.batch, "seeds": args.seeds}
    json.dump(out, open(args.out, "w"), indent=1)
    print(json.dumps(out["summary_mae"]), "synthetic", len(synth), "real pairs", len(real))


if __name__ == "__main__":
    main()
