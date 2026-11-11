import sys
import io
import re
import zipfile
import requests
from common import *

SOURCE = "kenney"
SKIP_SLUGS = set()


def slugs():
    out = []
    for page in ["", "/page:2", "/page:3", "/page:4", "/page:5"]:
        h = requests.get(f"https://kenney.nl/assets/category:3D{page}", headers=UA, timeout=60).text
        out += re.findall(r"kenney.nl/assets/([a-z0-9-]+)'", h)
    return sorted(set(out) - {"category"})


def pack_url(slug):
    s = requests.Session()
    s.headers.update(UA)
    s.get(f"https://kenney.nl/assets/{slug}", timeout=60)
    ident = f"assets/{slug}".encode().hex()
    r = s.get(f"https://kenney.nl/download-link/{ident}", headers={"Referer": f"https://kenney.nl/assets/{slug}"}, timeout=60)
    return r.json().get("url")


def download():
    check_space(5, SOURCE)
    rd = raw_dir(SOURCE)
    sl = slugs()
    (rd / "slugs.json").write_text(json.dumps(sl))
    for slug in sl:
        z = rd / f"{slug}.zip"
        if z.exists():
            continue
        try:
            url = pack_url(slug)
            if not url:
                print("no link", slug)
                continue
            fetch(url, z)
        except Exception as e:
            print("fail", slug, e)


def normalise():
    rd = raw_dir(SOURCE)
    jobs = []
    for z in sorted(rd.glob("*.zip")):
        slug = z.stem
        ex = rd / slug
        if not ex.exists():
            try:
                zipfile.ZipFile(z).extractall(ex)
            except Exception as e:
                print("bad zip", z, e)
                continue
        files = [p for p in ex.rglob("*") if p.suffix.lower() in (".glb", ".obj", ".gltf", ".fbx") and "Previews" not in p.parts]
        glbs = [p for p in files if p.suffix.lower() == ".glb"]
        if glbs:
            files = glbs
        else:
            objs = [p for p in files if p.suffix.lower() == ".obj"]
            files = objs or [p for p in files if p.suffix.lower() == ".gltf"]
        for p in files:
            oid = f"{slug}/{p.relative_to(ex).with_suffix('')}".replace("/", "__")
            row = row_base(SOURCE, oid, f"https://kenney.nl/assets/{slug}", "CC0-1.0", True,
                           "Kenney (kenney.nl), CC0, attribution optional", slug)
            jobs.append(dict(src=p, out_glb=out_dir(SOURCE) / "glb" / f"{oid}.glb", category=slug, row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
