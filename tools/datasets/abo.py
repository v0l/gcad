import csv
import gzip
import random
import sys
import tarfile
from common import *

SOURCE = "abo"
S3 = "https://amazon-berkeley-objects.s3.amazonaws.com"
CAP = 1500
MAX_FACES = 100000


def metadata():
    rd = raw_dir(SOURCE)
    fetch(f"{S3}/archives/abo-listings.tar", rd / "abo-listings.tar")
    fetch(f"{S3}/3dmodels/metadata/3dmodels.csv.gz", rd / "3dmodels.csv.gz")
    fetch(f"{S3}/images/metadata/images.csv.gz", rd / "images.csv.gz")
    if not (rd / "listings").exists():
        tarfile.open(rd / "abo-listings.tar").extractall(rd)


def english(v):
    if isinstance(v, list):
        for x in v:
            if str(x.get("language_tag", "")).startswith("en"):
                return x.get("value")
        return v[0].get("value") if v else None
    return v


def listings():
    rd = raw_dir(SOURCE)
    out = {}
    for f in sorted((rd / "listings" / "metadata").glob("*.json.gz")):
        for line in gzip.open(f, "rt"):
            j = json.loads(line)
            mid = j.get("3dmodel_id")
            if mid and mid not in out:
                pt = j.get("product_type") or [{}]
                out[mid] = {"name": english(j.get("item_name")), "type": pt[0].get("value", "unknown"),
                            "image": j.get("main_image_id")}
    return out


def select():
    rd = raw_dir(SOURCE)
    models = [r for r in csv.DictReader(gzip.open(rd / "3dmodels.csv.gz", "rt")) if int(r["faces"]) <= MAX_FACES]
    imgs = {r["image_id"]: r["path"] for r in csv.DictReader(gzip.open(rd / "images.csv.gz", "rt"))}
    lst = listings()
    rng = random.Random(548)
    rng.shuffle(models)
    chosen = []
    for m in models:
        info = lst.get(m["3dmodel_id"])
        if not info or info["image"] not in imgs:
            continue
        chosen.append({"id": m["3dmodel_id"], "path": m["path"], "faces": int(m["faces"]), "name": info["name"],
                       "category": info["type"].lower(), "image": imgs[info["image"]]})
        if len(chosen) >= CAP:
            break
    write_jsonl(rd / "selection.jsonl", chosen)
    print(len(chosen), "selected")


def download():
    rd = raw_dir(SOURCE)
    sel = read_jsonl(rd / "selection.jsonl")
    check_space(30, SOURCE)
    jobs = []
    for c in sel:
        jobs.append(dict(url=f"{S3}/3dmodels/original/{c['path']}", dest=rd / "glb" / f"{c['id']}.glb"))
        jobs.append(dict(url=f"{S3}/images/small/{c['image']}", dest=rd / "photo" / f"{c['id']}.jpg"))
    for e in fetch_many(jobs, 8)[:20]:
        print("fail", e)


def normalise():
    from PIL import Image
    rd = raw_dir(SOURCE)
    jobs = []
    for c in read_jsonl(rd / "selection.jsonl"):
        p = rd / "glb" / f"{c['id']}.glb"
        j = rd / "photo" / f"{c['id']}.jpg"
        if not p.exists():
            continue
        png = out_dir(SOURCE) / "photo" / f"{c['id']}.png"
        has_photo = False
        if j.exists():
            png.parent.mkdir(parents=True, exist_ok=True)
            try:
                Image.open(j).convert("RGB").save(png)
                has_photo = True
            except Exception:
                pass
        row = row_base(SOURCE, c["id"], f"https://amazon-berkeley-objects.s3.amazonaws.com/index.html#{c['id']}",
                       "CC-BY-4.0", True, f"Amazon Berkeley Objects ({c['id']}), CC BY 4.0, Amazon.com", c["category"],
                       has_photo=has_photo)
        if has_photo:
            row["paths"]["photo"] = str(png.relative_to(DATA))
        jobs.append(dict(src=p, out_glb=out_dir(SOURCE) / "glb" / f"{c['id']}.glb", category=c["category"], row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["metadata", "select", "download", "normalise"]:
        globals()[step]()
