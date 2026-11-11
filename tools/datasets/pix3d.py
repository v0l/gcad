import sys
import zipfile
from PIL import Image
from common import *

SOURCE = "pix3d"
URL = "http://pix3d.csail.mit.edu/data/pix3d.zip"
MAX_PHOTOS_PER_MODEL = 3


def download():
    check_space(10, SOURCE)
    rd = raw_dir(SOURCE)
    aria(URL, rd / "pix3d.zip")
    if not (rd / "pix3d.json").exists():
        with zipfile.ZipFile(rd / "pix3d.zip") as z:
            z.extractall(rd)


def normalise():
    rd = raw_dir(SOURCE)
    entries = json.load(open(rd / "pix3d.json"))
    models = {}
    for e in entries:
        models.setdefault(e["model"], []).append(e)
    jobs = []
    for model, es in sorted(models.items()):
        src = rd / model
        if not src.exists():
            continue
        oid = model.replace("/", "__").rsplit(".", 1)[0]
        cat = es[0]["category"]
        photos = []
        for i, e in enumerate(es[:MAX_PHOTOS_PER_MODEL]):
            p = rd / e["img"]
            if not p.exists():
                continue
            png = out_dir(SOURCE) / "photo" / f"{oid}_{i}.png"
            png.parent.mkdir(parents=True, exist_ok=True)
            try:
                Image.open(p).convert("RGB").save(png)
                photos.append(str(png.relative_to(DATA)))
            except Exception:
                pass
        row = row_base(SOURCE, oid, "http://pix3d.csail.mit.edu/", "CC-BY-4.0", False,
                       "Pix3D (Sun et al., CVPR 2018), CC BY 4.0 per dataset page; IKEA product designs, kept out of the commercial set",
                       cat, has_photo=bool(photos), photo_count=len(es))
        if photos:
            row["paths"]["photo"] = photos
        jobs.append(dict(src=src, out_glb=out_dir(SOURCE) / "glb" / f"{oid}.glb", category=cat, row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
