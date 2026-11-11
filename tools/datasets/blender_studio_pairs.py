import re
import sys
import subprocess
import zipfile
import requests
from PIL import Image
from common import *
import blender_studio as bs

SOURCE = "blender-studio"
BLENDER = DATA / "tools" / "blender-4.2.3-linux-x64" / "blender"
SCRIPT = Path(__file__).with_name("blend_to_glb.py")

PAIRS = {
    "lunte": {"character": "lunte/v4", "film": "Settlers", "concepts": ["Lunte Turnaround", "Lunte Color Exploration", "Lunte Mask Exploration", "Lunte Hair Styles and Notes"]},
    "gabby": {"character": "gabby/v3", "film": "Settlers", "concepts": ["Gabby Concept", "Gabby Face Proportions", "Gabby Goggles", "Gabby Expressions"]},
    "pip": {"character": "pip/v3", "film": "Settlers", "concepts": ["Pip Concept & Variations"]},
    "phileas": {"character": "phileas/v3", "film": "Settlers", "concepts": ["Phileas Filetson Concept", "Phileas's Backpack Design"]},
}
ASSET_PAIRS = {
    "bbb-bird": {"model": ("big-buck-bunny", "Bird"), "film": "Big Buck Bunny", "concepts": [("big-buck-bunny", "Big Buck Bunny - Character beauty render")]},
    "bbb-frankie": {"model": ("big-buck-bunny", "Frankie"), "film": "Big Buck Bunny", "concepts": [("big-buck-bunny", "Big Buck Bunny - Character beauty render")]},
}


def sess():
    s = requests.Session()
    s.headers.update(UA)
    return s


def download():
    check_space(5, SOURCE)
    rd = raw_dir(SOURCE)
    if not (rd / "assets.json").exists():
        bs.crawl()
    chars = {c["slug"]: c for c in json.loads((rd / "characters.json").read_text())}
    assets = json.loads((rd / "assets.json").read_text())
    s = sess()
    for c in chars.values():
        if c["blend"]:
            ext = c["blend"].rsplit(".", 1)[-1]
            fetch(BASE + c["blend"], rd / "characters" / f"{c['slug'].replace('/', '_')}.{ext}", session=s)
    for a in assets:
        if a["locked"] or not a.get("download"):
            continue
        ext = a["download"].rsplit(".", 1)[-1].lower()
        if ext in ("png", "jpg", "jpeg", "blend", "kra"):
            fetch(BASE + a["download"], rd / "assets" / f"{a['asset']}.{ext}", session=s)


BASE = bs.BASE


def blend_to_glb(blend, glb):
    glb.parent.mkdir(parents=True, exist_ok=True)
    if glb.exists():
        return True
    r = subprocess.run([str(BLENDER), "-b", "--factory-startup", "-P", str(SCRIPT), "--", str(blend), str(glb)],
                       capture_output=True, text=True, timeout=600)
    return glb.exists()


def to_png(src, dst):
    dst.parent.mkdir(parents=True, exist_ok=True)
    im = Image.open(src)
    im.thumbnail((2048, 2048))
    im.convert("RGB").save(dst)


def character_blend(rd, slug, c):
    ext = c["blend"].rsplit(".", 1)[-1]
    f = rd / "characters" / f"{slug.replace('/', '_')}.{ext}"
    if ext == "zip" and f.exists():
        d = rd / "characters" / slug.replace("/", "_")
        if not d.exists():
            zipfile.ZipFile(f).extractall(d)
        blends = sorted(d.rglob("*.blend"), key=lambda p: -p.stat().st_size)
        return blends[0] if blends else None
    return f if f.exists() else None


def normalise():
    rd = raw_dir(SOURCE)
    od = out_dir(SOURCE)
    chars = {c["slug"]: c for c in json.loads((rd / "characters.json").read_text())}
    assets = json.loads((rd / "assets.json").read_text())
    by_title = {(a["project"], a["title"]): a for a in assets if not a["locked"] and a.get("download")}
    rows, dropped = [], []

    def add(oid, blend, category, url, film, concept_files, license_id="CC-BY-4.0", extra=None):
        glb = od / "glb" / f"{oid}.glb"
        try:
            if not blend_to_glb(blend, glb):
                raise ValueError("blender export produced no mesh")
            st = mesh_stats(glb)
        except Exception as e:
            dropped.append({"id": f"{SOURCE}:{oid}", "source": SOURCE, "src": str(blend), "reason": f"{type(e).__name__}: {str(e)[:160]}"})
            return
        row = row_base(SOURCE, oid, url, license_id, True,
                       f"{film}, (CC-BY) Blender Foundation | studio.blender.org", category,
                       has_concept=bool(concept_files))
        row["paths"] = {"glb": str(glb.relative_to(DATA)), "original": str(Path(blend).relative_to(DATA))}
        pngs = []
        for i, cf in enumerate(concept_files):
            p = od / "concept" / f"{oid}_{i}.png"
            try:
                to_png(cf, p)
                pngs.append(str(p.relative_to(DATA)))
            except Exception:
                pass
        row["has_concept"] = bool(pngs)
        if pngs:
            row["paths"]["concept"] = pngs
        row.update({"triangles": st["triangles"], "watertight": st["watertight"], "parts": st["parts"],
                    "bbox_min": st["bbox_min"], "bbox_max": st["bbox_max"]})
        row.update(fit_label(st, category))
        if extra:
            row.update(extra)
        rows.append(row)

    paired = set()
    for name, pr in PAIRS.items():
        c = chars.get(pr["character"])
        if not c or not c["blend"]:
            continue
        files = []
        for t in pr["concepts"]:
            a = by_title.get(("settlers", t))
            if a:
                ext = a["download"].rsplit(".", 1)[-1].lower()
                f = rd / "assets" / f"{a['asset']}.{ext}"
                if f.exists():
                    files.append(f)
        paired.add(pr["character"])
        src = character_blend(rd, pr["character"], c)
        if not src:
            continue
        add(f"character-{name}", src, "character", f"{BASE}/characters/{pr['character']}/", pr["film"], files)
    for name, pr in ASSET_PAIRS.items():
        m = by_title.get(pr["model"])
        if not m:
            continue
        files = []
        for proj, t in pr["concepts"]:
            a = by_title.get((proj, t))
            if a:
                f = rd / "assets" / f"{a['asset']}.{a['download'].rsplit('.', 1)[-1].lower()}"
                if f.exists():
                    files.append(f)
        src = rd / "assets" / f"{m['asset']}.blend"
        if src.exists():
            add(name, src, "character", f"{BASE}/projects/{m['project']}/", pr["film"], files)
    for slug, c in chars.items():
        if slug in paired or not c["blend"] or not c["license"]:
            continue
        src = character_blend(rd, slug, c)
        if not src:
            continue
        add(f"character-{slug.replace('/', '_')}", src, "character", f"{BASE}/characters/{slug}/", "Blender Studio", [])
    rows.sort(key=lambda r: r["id"])
    write_jsonl(od / "manifest.jsonl", rows)
    write_jsonl(od / "dropped.jsonl", dropped)
    print(len(rows), "kept", len(dropped), "dropped", sum(r["has_concept"] for r in rows), "with concept")


if __name__ == "__main__":
    for step in sys.argv[1:] or ["crawl", "download", "normalise"]:
        globals()[step]() if step != "crawl" else bs.crawl()
