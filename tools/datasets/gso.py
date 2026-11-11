import sys
import zipfile
import requests
from common import *

SOURCE = "gso"
API = "https://fuel.gazebosim.org/1.0/GoogleResearch/models"


def listing():
    out, page = [], 1
    while True:
        resp = requests.get(API, params={"per_page": 100, "page": page}, headers=UA, timeout=60)
        r = resp.json() if resp.status_code == 200 else []
        if not r:
            return out
        out += r
        page += 1


def download():
    check_space(10, SOURCE)
    rd = raw_dir(SOURCE)
    models = listing()
    (rd / "models.json").write_text(json.dumps(models))
    jobs = [dict(url=f"{API}/{m['name']}.zip", dest=rd / "zip" / f"{m['name']}.zip") for m in models]
    for e in fetch_many(jobs, 8)[:20]:
        print("fail", e)


def normalise():
    rd = raw_dir(SOURCE)
    models = json.loads((rd / "models.json").read_text())
    jobs = []
    for m in models:
        name = m["name"]
        z = rd / "zip" / f"{name}.zip"
        if not z.exists():
            continue
        ex = rd / "obj" / name
        if not ex.exists():
            try:
                zipfile.ZipFile(z).extractall(ex)
            except Exception as e:
                print("bad zip", name, e)
                continue
        obj = ex / "meshes" / "model.obj"
        if not obj.exists():
            continue
        photo = ex / "thumbnails" / "0.jpg"
        row = row_base(SOURCE, name, f"https://app.gazebosim.org/GoogleResearch/fuel/models/{name}", "CC-BY-4.0", True,
                       f"{name} from Google Scanned Objects by Google Research, CC BY 4.0", "household-scan",
                       has_photo=photo.exists())
        if photo.exists():
            row["paths"]["photo"] = str(photo.relative_to(DATA))
        jobs.append(dict(src=obj, out_glb=out_dir(SOURCE) / "glb" / f"{name}.glb", category="household-scan", row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
