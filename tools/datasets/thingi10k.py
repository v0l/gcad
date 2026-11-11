import csv
import io
import sys
import requests
from common import *

SOURCE = "thingi10k"
REPO = "https://huggingface.co/datasets/Thingi10K/Thingi10K/resolve/main"
COMMERCIAL = {
    "Creative Commons - Attribution": "CC-BY-4.0",
    "Creative Commons - Public Domain Dedication": "CC0-1.0",
    "Public Domain": "LicenseRef-PublicDomain",
    "BSD License": "BSD-3-Clause",
}
OTHER = {
    "Creative Commons - Attribution - Share Alike": "CC-BY-SA-4.0",
    "Creative Commons - Attribution - Non-Commercial": "CC-BY-NC-4.0",
    "Attribution - Non-Commercial - Share Alike": "CC-BY-NC-SA-4.0",
    "Attribution - Non-Commercial - No Derivatives": "CC-BY-NC-ND-4.0",
    "Creative Commons - Attribution - No Derivatives": "CC-BY-ND-4.0",
    "GNU - GPL": "GPL-3.0-only",
    "GNU - LGPL": "LGPL-3.0-only",
}


def metadata():
    rd = raw_dir(SOURCE)
    for name in ("input_summary.csv", "contextual_data.csv"):
        fetch(f"{REPO}/metadata/{name}", rd / name)


def download():
    check_space(15, SOURCE)
    metadata()
    rd = raw_dir(SOURCE)
    rows = list(csv.DictReader(open(rd / "input_summary.csv")))
    jobs = [dict(url=f"{REPO}/raw_meshes/{r['ID']}.stl", dest=rd / "raw_meshes" / f"{r['ID']}.stl") for r in rows]
    for e in fetch_many(jobs, 8)[:20]:
        print("fail", e)


def normalise():
    rd = raw_dir(SOURCE)
    ctx = {r["Thing ID"]: r for r in csv.DictReader(open(rd / "contextual_data.csv"))}
    jobs = []
    for r in csv.DictReader(open(rd / "input_summary.csv")):
        p = rd / "raw_meshes" / f"{r['ID']}.stl"
        if not p.exists():
            continue
        lic_name = r["License"]
        spdx = COMMERCIAL.get(lic_name) or OTHER.get(lic_name) or "LicenseRef-Unknown"
        c = ctx.get(r["Thing ID"], {})
        cat = c.get("Category", "None")
        cat = "3d-printing" if cat in ("None", "") else cat
        url = f"https://www.thingiverse.com/thing:{r['Thing ID']}"
        att = f"\"{(c.get('Name') or '').strip()}\" by {c.get('Author', 'unknown')} on Thingiverse, {lic_name}"
        row = row_base(SOURCE, r["ID"], url, spdx, lic_name in COMMERCIAL, att, cat)
        jobs.append(dict(src=p, out_glb=out_dir(SOURCE) / "glb" / f"{r['ID']}.glb", category=cat, row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["metadata", "download", "normalise"]:
        globals()[step]()
