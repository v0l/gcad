import argparse
import json
import os

import numpy as np
from PIL import Image
from scipy import ndimage

SIZE = 256


def render_mask(path, size=SIZE):
    a = Image.open(path).convert("RGBA").resize((size, size), Image.LANCZOS)
    return np.asarray(a)[:, :, 3] > 127


def concept_mask(path, size=SIZE, threshold=28.0):
    im = np.asarray(Image.open(path).convert("RGB").resize((size, size), Image.LANCZOS)).astype(np.float32)
    k = max(2, size // 32)
    corners = np.concatenate([im[:k, :k].reshape(-1, 3), im[:k, -k:].reshape(-1, 3),
                              im[-k:, :k].reshape(-1, 3), im[-k:, -k:].reshape(-1, 3)])
    bg = np.median(corners, axis=0)
    mask = np.linalg.norm(im - bg, axis=2) > threshold
    mask = ndimage.binary_opening(mask, iterations=1)
    mask = ndimage.binary_closing(mask, iterations=2)
    return ndimage.binary_fill_holes(mask)


def iou(a, b):
    union = np.logical_or(a, b).sum()
    if union == 0:
        return 0.0
    return float(np.logical_and(a, b).sum() / union)


def best_iou(concept, render, shift=6):
    best = 0.0
    for dy in range(-shift, shift + 1, 3):
        for dx in range(-shift, shift + 1, 3):
            best = max(best, iou(concept, np.roll(np.roll(render, dy, 0), dx, 1)))
    return best


def score(concept_path, render_path):
    return best_iou(concept_mask(concept_path), render_mask(render_path))


def filter_generated(meta_path, min_iou, kept_path, dropped_path):
    kept, dropped = [], []
    for line in open(meta_path):
        g = json.loads(line)
        s = score(g["concept"], g["render"])
        g["silhouette_iou"] = round(s, 4)
        if s >= min_iou:
            kept.append(g)
        else:
            g["drop_reason"] = f"silhouette_iou {s:.3f} < {min_iou}"
            dropped.append(g)
    with open(kept_path, "w") as f:
        for g in kept:
            f.write(json.dumps(g) + "\n")
    with open(dropped_path, "w") as f:
        for g in dropped:
            f.write(json.dumps(g) + "\n")
    return kept, dropped


def main():
    base = os.path.expanduser("~/gcad-datasets/out/synthetic/")
    ap = argparse.ArgumentParser()
    ap.add_argument("--meta", default=base + "generated.jsonl")
    ap.add_argument("--min-iou", type=float, default=0.75)
    ap.add_argument("--kept", default=base + "kept.jsonl")
    ap.add_argument("--dropped", default=base + "dropped.jsonl")
    args = ap.parse_args()
    kept, dropped = filter_generated(args.meta, args.min_iou, args.kept, args.dropped)
    print(len(kept), "kept", len(dropped), "dropped")
    for g in dropped:
        print("drop", g["id"], g["drop_reason"])


if __name__ == "__main__":
    main()
