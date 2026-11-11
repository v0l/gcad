import re
import html
import sys
import requests
from common import *

SOURCE = "blender-studio"
BASE = "https://studio.blender.org"
PROJECTS = ["spring", "charge", "sprite-fright", "coffee-run", "sintel", "agent-327", "wing-it", "cosmos-laundromat", "tears-of-steel", "big-buck-bunny", "elephants-dream", "caminandes", "hero", "settlers", "heist", "dogwalk", "pets", "project-gold", "storm"]
CONCEPT_WORDS = ("concept", "design", "sketch", "turnaround", "character sheet", "model sheet", "expression", "reference")


def get(url):
    return requests.get(url, headers=UA, timeout=60).text


def characters():
    h = get(f"{BASE}/characters/")
    out = []
    for slug in sorted(set(re.findall(r'href="/characters/([a-z0-9-]+/v\d+)/"', h))):
        p = get(f"{BASE}/characters/{slug}/")
        blend = re.search(r'href="(/download-source/files/[^"]+\.(?:blend|zip))"', p)
        lic = re.search(r"License:?\s*(CC-BY|CC0|CC-BY-SA)", re.sub(r"<[^>]*>", " ", p))
        title = re.search(r"<title>([^<]*)</title>", p)
        film = re.search(r"project-name[^>]*>\s*([^<]+)<", p)
        out.append({"slug": slug, "blend": blend.group(1) if blend else None,
                    "license": lic.group(1) if lic else None,
                    "title": html.unescape(title.group(1)).strip() if title else slug})
    return out


def gallery_assets(project):
    out = []
    try:
        g = get(f"{BASE}/projects/{project}/gallery/")
    except Exception:
        return out
    cols = sorted(set(re.findall(rf'href="/projects/{project}/([0-9a-f]{{24}})/"', g)))
    for c in cols:
        h = get(f"{BASE}/projects/{project}/{c}/")
        for m in re.finditer(r'<div class="cards-item-file[^"]*"[^>]*>\s*<a[^>]*title="([^"]*)" data-asset-id="(\d+)"', h):
            blk = h[m.start(): m.start() + 900]
            out.append({"project": project, "collection": c, "asset": m.group(2),
                        "title": html.unescape(m.group(1)), "locked": "file-lock-text" in blk})
    return out


def asset_detail(aid):
    h = get(f"{BASE}/projects/api/assets/{aid}/")
    dl = re.search(r'href="(/download-source/files/[^"]+)"', h)
    lic = re.search(r"License[^<]*</[^>]*>\s*<[^>]*>\s*([A-Za-z0-9-]+)", h) or re.search(r"License:\s*([A-Za-z0-9-]+)", re.sub(r"<[^>]*>", " ", h))
    return dl.group(1) if dl else None, lic.group(1) if lic else None


def crawl_characters():
    rd = raw_dir(SOURCE)
    chars = characters()
    (rd / "characters.json").write_text(json.dumps(chars, indent=1))
    print(len(chars), "characters", sum(1 for c in chars if c["blend"]), "with files")


def crawl():
    rd = raw_dir(SOURCE)
    chars = characters()
    (rd / "characters.json").write_text(json.dumps(chars, indent=1))
    assets = []
    for p in PROJECTS:
        assets += gallery_assets(p)
    for a in assets:
        if not a["locked"]:
            a["download"], a["license"] = asset_detail(a["asset"])
    (rd / "assets.json").write_text(json.dumps(assets, indent=1))
    print(len(chars), "characters,", len(assets), "gallery assets,", sum(not a["locked"] for a in assets), "open")


if __name__ == "__main__":
    for step in sys.argv[1:] or ["crawl"]:
        globals()[step]()
