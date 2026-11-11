import sys
import requests
from common import *

SOURCE = "polyhaven"
CAP_NOTE = "1k gltf with textures"


def download():
    check_space(5, SOURCE)
    rd = raw_dir(SOURCE)
    assets = requests.get("https://api.polyhaven.com/assets?t=models", headers=UA, timeout=60).json()
    (rd / "assets.json").write_text(json.dumps(assets))
    jobs = []
    for aid in assets:
        files = requests.get(f"https://api.polyhaven.com/files/{aid}", headers=UA, timeout=60).json()
        try:
            g = files["gltf"]["1k"]["gltf"]
        except (KeyError, TypeError):
            print("no gltf", aid)
            continue
        d = rd / aid
        jobs.append(dict(url=g["url"], dest=d / Path(g["url"]).name, md5=g.get("md5")))
        for rel, f in g["include"].items():
            jobs.append(dict(url=f["url"], dest=d / rel, md5=f.get("md5")))
    errs = fetch_many(jobs, 8)
    for e in errs:
        print("fail", e)


def normalise():
    rd = raw_dir(SOURCE)
    assets = json.loads((rd / "assets.json").read_text())
    jobs = []
    for aid, a in assets.items():
        gl = list((rd / aid).glob("*.gltf"))
        if not gl:
            continue
        cat = a["categories"][0] if a.get("categories") else "prop"
        authors = ", ".join(a.get("authors", {}).keys())
        row = row_base(SOURCE, aid, f"https://polyhaven.com/a/{aid}", "CC0-1.0", True,
                       f"{a['name']} by {authors} (Poly Haven, CC0, attribution optional)", cat)
        jobs.append(dict(src=gl[0], out_glb=out_dir(SOURCE) / "glb" / f"{aid}.glb", category=cat, row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
