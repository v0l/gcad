import json
import os
import sys

import numpy as np
from PIL import Image

import filter_iou

S = os.path.expanduser("~/gcad-datasets/out/synthetic/")


def load(p):
    return {json.loads(l)["id"]: json.loads(l) for l in open(S + p)}


def ious(gen):
    return {k: filter_iou.score(g["concept"], g["render"]) for k, g in gen.items()}


def main():
    h, g = load("generated_img2img.jsonl"), load("generated_controlnet.jsonl")
    a, b = ious(h), ious(g)
    for n, d in (("img2img", a), ("controlnet", b)):
        v = np.array(list(d.values()))
        print(n, "mean %.3f median %.3f p10 %.3f kept %d" % (v.mean(), np.median(v), np.percentile(v, 10), (v >= .75).sum()))
    ids = sorted(k for k in a if k in b and a[k] > .6)[::3][:12]
    W = Image.new("RGB", (6 * 256, 6 * 256), "white")
    for i, k in enumerate(ids):
        r = Image.open(g[k]["render"]).convert("RGBA")
        bg = Image.new("RGBA", r.size, (236, 236, 232, 255))
        bg.alpha_composite(r)
        for j, im in enumerate([bg.convert("RGB"), Image.open(h[k]["concept"]).convert("RGB"), Image.open(g[k]["concept"]).convert("RGB")]):
            W.paste(im.resize((256, 256)), (((i % 2) * 3 + j) * 256, (i // 2) * 256))
    W.save(sys.argv[1])
    print(ids)


main()
