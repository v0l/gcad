import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from multiprocessing import Pool
from pathlib import Path

DATA = Path(os.environ.get("GCAD_DATA", "/home/kieran/gcad-datasets"))
BUDGET_GB = 300
MIN_FREE_GB = 100
UA = {"User-Agent": "Mozilla/5.0 gcad-datasets"}
MESH_EXT = {".glb", ".gltf", ".obj", ".stl", ".ply", ".off", ".fbx", ".blend", ".dae"}
MECHANICAL_CATEGORIES = {
    "cad", "mechanical", "tool", "hardware", "vehicle", "furniture", "building",
    "architecture", "prop", "weapon", "container", "appliance", "electronics",
}


def raw_dir(source):
    d = DATA / "raw" / source
    d.mkdir(parents=True, exist_ok=True)
    return d


def out_dir(source):
    d = DATA / "out" / source
    (d / "glb").mkdir(parents=True, exist_ok=True)
    return d


def used_gb():
    if not DATA.exists():
        return 0.0
    r = subprocess.run(["du", "-s", "--block-size=1G", "--exclude=venv", "--exclude=tools", str(DATA)],
                       capture_output=True, text=True)
    return float(r.stdout.split()[0])


def check_space(need_gb, label=""):
    if str(DATA.resolve()).startswith("/mnt/data"):
        sys.exit("refusing to use /mnt/data")
    free = shutil.disk_usage(DATA).free / 1e9
    used = used_gb()
    if free - need_gb < MIN_FREE_GB:
        sys.exit(f"{label}: only {free:.0f} GB free on /, need {need_gb} plus {MIN_FREE_GB} reserve")
    if used + need_gb > BUDGET_GB:
        sys.exit(f"{label}: {used:.0f} GB used plus {need_gb} GB would pass the {BUDGET_GB} GB budget")
    print(f"space ok: {free:.0f} GB free, {used:.0f} GB used of {BUDGET_GB}, adding up to {need_gb}")


def digest(path, algo="md5"):
    h = hashlib.new(algo)
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch(url, dest, md5=None, sha256=None, session=None, headers=None, retries=4):
    import requests
    dest = Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        if md5 and digest(dest, "md5") != md5:
            dest.unlink()
        elif sha256 and digest(dest, "sha256") != sha256:
            dest.unlink()
        else:
            return dest
    part = dest.with_name(dest.name + ".part")
    s = session or requests
    h = dict(UA)
    h.update(headers or {})
    for attempt in range(retries):
        try:
            have = part.stat().st_size if part.exists() else 0
            hh = dict(h)
            if have:
                hh["Range"] = f"bytes={have}-"
            with s.get(url, headers=hh, stream=True, timeout=60) as r:
                if r.status_code == 416:
                    part.rename(dest)
                    break
                r.raise_for_status()
                mode = "ab" if have and r.status_code == 206 else "wb"
                with open(part, mode) as f:
                    for chunk in r.iter_content(1 << 20):
                        f.write(chunk)
            part.rename(dest)
            break
        except Exception as e:
            if attempt == retries - 1:
                raise
            time.sleep(5 * (attempt + 1))
    if md5 and digest(dest, "md5") != md5:
        dest.unlink()
        raise IOError(f"md5 mismatch for {url}")
    if sha256 and digest(dest, "sha256") != sha256:
        dest.unlink()
        raise IOError(f"sha256 mismatch for {url}")
    return dest


def fetch_many(jobs, workers=8):
    errors = []

    def run(job):
        try:
            fetch(**job)
        except Exception as e:
            errors.append((job["url"], str(e)))

    with ThreadPoolExecutor(workers) as ex:
        list(ex.map(run, jobs))
    return errors


def aria(url, dest, connections=8):
    dest = Path(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["aria2c", "-c", "-x", str(connections), "-s", str(connections), "--console-log-level=warn",
                    "-d", str(dest.parent), "-o", dest.name, url], check=True)
    return dest


def hf_snapshot(repo, source, patterns, repo_type="dataset"):
    from huggingface_hub import snapshot_download
    return snapshot_download(repo, repo_type=repo_type, local_dir=str(raw_dir(source)),
                             allow_patterns=patterns, max_workers=8)


def load_scene(path):
    import trimesh
    return trimesh.load(str(path), force="scene", process=False)


def mesh_stats(path):
    import numpy as np
    import trimesh
    scene = load_scene(path)
    geoms = [g for g in scene.geometry.values() if isinstance(g, trimesh.Trimesh) and len(g.faces)]
    if not geoms:
        raise ValueError("no triangles")
    tris = int(sum(len(g.faces) for g in geoms))
    if tris == 0:
        raise ValueError("empty mesh")
    dumped = scene.dump(concatenate=False) if len(scene.graph.nodes_geometry) > 1 else geoms
    dumped = [g for g in dumped if isinstance(g, trimesh.Trimesh) and len(g.faces)]
    allv = np.concatenate([g.vertices for g in dumped])
    if not np.isfinite(allv).all():
        raise ValueError("non finite vertices")
    lo, hi = allv.min(0), allv.max(0)
    if float((hi - lo).max()) <= 0:
        raise ValueError("degenerate bounding box")
    parts = 0
    watertight = True
    for g in dumped:
        m = g.copy()
        m.merge_vertices()
        if not m.is_watertight:
            watertight = False
        if len(m.faces) <= 300000:
            try:
                parts += max(1, len(trimesh.graph.connected_components(m.face_adjacency, nodes=range(len(m.faces)))))
            except Exception:
                parts += 1
        else:
            parts += 1
    area = 0.0
    axis = 0.0
    for g in dumped:
        a = g.area_faces
        n = g.face_normals
        area += float(a.sum())
        axis += float(a[(np.abs(n).max(1) > 0.99)].sum())
    return {
        "triangles": tris,
        "parts": int(parts),
        "bbox_min": [round(float(x), 5) for x in lo],
        "bbox_max": [round(float(x), 5) for x in hi],
        "watertight": bool(watertight),
        "axis_aligned_area": round(axis / area, 4) if area > 0 else 0.0,
    }


def to_glb(src, dst):
    src, dst = Path(src), Path(dst)
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists() and dst.stat().st_size > 0:
        return dst
    if src.suffix.lower() == ".glb":
        try:
            os.link(src, dst)
        except OSError:
            shutil.copy2(src, dst)
        return dst
    scene = load_scene(src)
    scene.export(str(dst), file_type="glb")
    return dst


def fit_label(stats, category="", cad=False):
    tris = stats["triangles"]
    parts = stats["parts"]
    mech = cad or stats["axis_aligned_area"] >= 0.45 or category.lower() in MECHANICAL_CATEGORIES
    kind = "mechanical" if mech else "organic"
    if tris <= 1500:
        t = 1.0
    elif tris <= 10000:
        t = 0.75
    elif tris <= 50000:
        t = 0.5
    elif tris <= 200000:
        t = 0.25
    else:
        t = 0.1
    if parts <= 5:
        p = 1.0
    elif parts <= 30:
        p = 0.75
    elif parts <= 150:
        p = 0.5
    else:
        p = 0.25
    k = 1.0 if mech else 0.6
    score = round(t * p * k if not cad else max(t, 0.5) * p, 3)
    if cad and tris > 50000:
        score = min(score, 0.5)
    label = "high" if score >= 0.6 else "medium" if score >= 0.25 else "low"
    return {"gcad_fit": label, "gcad_fit_score": score, "shape_kind": kind}


MANIFEST_FIELDS = [
    "id", "source", "url", "license", "commercial_ok", "attribution", "category", "paths",
    "has_concept", "has_build_history", "has_photo", "triangles", "watertight",
]


def normalise_one(job):
    try:
        src = Path(job["src"])
        glb = to_glb(src, Path(job["out_glb"]))
        st = mesh_stats(glb)
        fit = fit_label(st, job.get("category", ""), job.get("cad", False))
        row = dict(job["row"])
        row["paths"] = dict(row.get("paths", {}))
        row["paths"]["original"] = str(src.relative_to(DATA))
        row["paths"]["glb"] = str(Path(job["out_glb"]).relative_to(DATA))
        row.update({"triangles": st["triangles"], "watertight": st["watertight"], "parts": st["parts"],
                    "bbox_min": st["bbox_min"], "bbox_max": st["bbox_max"]})
        row.update(fit)
        return ("ok", row)
    except Exception as e:
        return ("drop", {"id": job["row"]["id"], "source": job["row"]["source"], "src": str(job["src"]),
                         "reason": f"{type(e).__name__}: {str(e)[:200]}"})


def normalise_all(source, jobs, workers=8):
    od = out_dir(source)
    ok, dropped = [], []
    with Pool(workers) as pool:
        for status, row in pool.imap_unordered(normalise_one, jobs, chunksize=4):
            (ok if status == "ok" else dropped).append(row)
    ok.sort(key=lambda r: r["id"])
    write_jsonl(od / "manifest.jsonl", ok)
    write_jsonl(od / "dropped.jsonl", dropped)
    print(f"{source}: {len(ok)} kept, {len(dropped)} dropped")
    return ok, dropped


def row_base(source, oid, url, lic, commercial, attribution, category, **flags):
    r = {
        "id": f"{source}:{oid}", "source": source, "url": url, "license": lic,
        "commercial_ok": commercial, "attribution": attribution, "category": category,
        "paths": {}, "has_concept": False, "has_build_history": False, "has_photo": False,
    }
    r.update(flags)
    return r


def write_jsonl(path, rows):
    with open(path, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")


def read_jsonl(path):
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


def du_gb(path):
    r = subprocess.run(["du", "-sb", str(path)], capture_output=True, text=True)
    return int(r.stdout.split()[0]) / 1e9 if r.stdout else 0.0
