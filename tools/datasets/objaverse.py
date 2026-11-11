import gzip
import random
import sys
from common import *

SOURCE = "objaverse"
REPO = "https://huggingface.co/datasets/allenai/objaverse/resolve/main"
CAP_CC0 = 4000
CAP_CCBY = 6000
MIN_FACES = 200
MAX_FACES = 100000
MAX_BYTES = 25 << 20
KEEP = {"cc0": ("CC0-1.0", False), "by": ("CC-BY-4.0", True)}


def metadata():
    rd = raw_dir(SOURCE)
    jobs = [dict(url=f"{REPO}/metadata/000-{i:03d}.json.gz", dest=rd / "metadata" / f"000-{i:03d}.json.gz") for i in range(160)]
    jobs.append(dict(url=f"{REPO}/object-paths.json.gz", dest=rd / "object-paths.json.gz"))
    for e in fetch_many(jobs, 8):
        print("fail", e)


def select():
    rd = raw_dir(SOURCE)
    paths = json.load(gzip.open(rd / "object-paths.json.gz"))
    cands = {"cc0": [], "by": []}
    for f in sorted((rd / "metadata").glob("*.json.gz")):
        for uid, m in json.load(gzip.open(f)).items():
            lic = m.get("license")
            if lic not in KEEP or uid not in paths:
                continue
            glb = (m.get("archives") or {}).get("glb") or {}
            faces = glb.get("faceCount") or m.get("faceCount") or 0
            size = glb.get("size") or 0
            if not (MIN_FACES <= faces <= MAX_FACES) or size > MAX_BYTES or m.get("isAgeRestricted"):
                continue
            cands[lic].append({
                "uid": uid, "path": paths[uid], "name": m.get("name"), "faces": faces, "size": size,
                "category": (m.get("categories") or [{"name": "uncategorised"}])[0]["name"],
                "author": (m.get("user") or {}).get("displayName") or (m.get("user") or {}).get("username") or "unknown",
                "url": m.get("viewerUrl"), "license": lic, "tags": [t["name"] for t in (m.get("tags") or [])][:12],
            })
    rng = random.Random(548)
    chosen = []
    for lic, cap in (("cc0", CAP_CC0), ("by", CAP_CCBY)):
        c = cands[lic]
        rng.shuffle(c)
        print(lic, "candidates", len(c), "taking", min(cap, len(c)))
        chosen += c[:cap]
    write_jsonl(rd / "selection.jsonl", chosen)
    gb = sum(c["size"] for c in chosen) / 1e9
    print(f"{len(chosen)} objects, about {gb:.1f} GB")


def download():
    sel = read_jsonl(raw_dir(SOURCE) / "selection.jsonl")
    gb = sum(c["size"] for c in sel) / 1e9
    check_space(int(gb * 1.1) + 1, SOURCE)
    rd = raw_dir(SOURCE)
    jobs = [dict(url=f"{REPO}/{c['path']}", dest=rd / "glb" / f"{c['uid']}.glb") for c in sel]
    for e in fetch_many(jobs, 8)[:20]:
        print("fail", e)


def normalise():
    rd = raw_dir(SOURCE)
    jobs = []
    for c in read_jsonl(rd / "selection.jsonl"):
        p = rd / "glb" / f"{c['uid']}.glb"
        if not p.exists():
            continue
        lic, _ = KEEP[c["license"]]
        att = f"\"{c['name']}\" by {c['author']} on Sketchfab, {lic}"
        row = row_base(SOURCE, c["uid"], c["url"], lic, True, att, c["category"], tags=c["tags"])
        jobs.append(dict(src=p, out_glb=out_dir(SOURCE) / "glb" / f"{c['uid']}.glb", category=c["category"], row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["metadata", "select", "download", "normalise"]:
        globals()[step]()
