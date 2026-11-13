import argparse
import hashlib
import json
import os
import random
import subprocess
import sys
import tempfile
import time
from collections import Counter
from multiprocessing import Pool
from pathlib import Path

from PIL import Image

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from shape_distance import compare_meshes, load_mesh, verdict
from translate import ops_of, reason_of, translate

DATA = Path(os.environ.get("GCAD_DATA", "/home/kieran/gcad-datasets"))
OUT = DATA / "out" / "pairs"
SOURCE = DATA / "out" / "zero-to-cad"
GCAD = os.environ.get("GCAD_BIN", "gcad")
CELL = 600
SIZE = 512
VIEW_CELLS = {"iso": (0, 0), "top": (1, 0), "front": (0, 1), "right": (1, 1)}
PLAIN_VIEWS = ["iso", "top", "front", "right"]


def split_of(uid):
    bucket = int(hashlib.sha1(uid.encode()).hexdigest(), 16) % 100
    return "val" if bucket < 5 else "test" if bucket < 10 else "train"


def run(cmd, seconds):
    return subprocess.run(cmd, capture_output=True, text=True, timeout=seconds)


def crop_cells(sheet_path, names, dest, prefix):
    sheet = Image.open(sheet_path).convert("RGB")
    paths = {}
    for name in names:
        cx, cy = VIEW_CELLS[name]
        cell = sheet.crop((cx * CELL, cy * CELL, (cx + 1) * CELL, (cy + 1) * CELL))
        path = dest / f"{prefix}_{name}.png"
        cell.resize((SIZE, SIZE), Image.LANCZOS).save(path)
        paths[name] = path
    return paths


def render_views(gcad_path, dest, uid):
    dest.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        sheet = tmp / "sheet.png"
        r = run([GCAD, "render", str(gcad_path), str(sheet)], 120)
        if r.returncode != 0:
            raise RuntimeError("render:" + (r.stderr.strip().splitlines() or ["failed"])[-1][:80])
        views = crop_cells(sheet, PLAIN_VIEWS, dest, uid)
        flipped = tmp / "flipped.gcad"
        flipped.write_text(gcad_path.read_text() + "rotate 180 axis=z\n")
        sheet2 = tmp / "sheet2.png"
        r = run([GCAD, "render", str(flipped), str(sheet2)], 120)
        if r.returncode != 0:
            raise RuntimeError("render_flipped")
        back = Image.open(sheet2).convert("RGB").crop((CELL, CELL, 2 * CELL, 2 * CELL))
        left = dest / f"{uid}_left.png"
        back.resize((SIZE, SIZE), Image.LANCZOS).save(left)
        views["left"] = left
    return views


def process(args):
    pdir, = args
    uid = pdir.name
    result = {"id": uid}
    try:
        source = (pdir / "program.py").read_text()
        try:
            t = translate(source, 30)
        except BaseException as e:
            reason = reason_of(e)
            if reason.startswith("error"):
                reason += ":" + str(e)[:60].replace("\n", " ")
            result["status"] = "untranslated"
            result["reason"] = reason
            return result
        result["translated"] = True
        gcad_dir = OUT / "gcad" / uid[:2]
        gcad_dir.mkdir(parents=True, exist_ok=True)
        gcad_path = gcad_dir / f"{uid}.gcad"
        gcad_path.write_text(t["gcad"])
        with tempfile.TemporaryDirectory() as tmp:
            stl = Path(tmp) / "out.stl"
            r = run([GCAD, "export", str(gcad_path), str(stl)], 40)
            if r.returncode != 0:
                message = (r.stdout + r.stderr).strip().splitlines()
                result["status"] = "build_failed"
                result["reason"] = "build:" + (message[-1] if message else "failed")[:90]
                return result
            built = load_mesh(stl)
            source_mesh = load_mesh(pdir.parent.parent.parent / "glb" / uid[:2] / f"{uid}.glb")
            metrics = compare_meshes(source_mesh, built, volume_a=t["volume"])
        result["metrics"] = metrics
        if not verdict(metrics):
            result["status"] = "shape_mismatch"
            result["reason"] = "volume" if metrics["volume_error"] > 0.01 else "chamfer"
            gcad_path.unlink()
            return result
        views = render_views(gcad_path, OUT / "renders" / uid[:2], uid)
        text = t["gcad"]
        result.update({
            "status": "kept",
            "gcad": str(gcad_path.relative_to(DATA)),
            "renders": {k: str(v.relative_to(DATA)) for k, v in views.items()},
            "lines": len(text.strip().splitlines()),
            "ops": sorted(set(ops_of(text))),
            "op_counts": dict(Counter(ops_of(text))),
        })
        return result
    except BaseException as e:
        result["status"] = "failed"
        result["reason"] = reason_of(e) + ":" + str(e)[:80].replace("\n", " ")
        return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--jobs", type=int, default=12)
    parser.add_argument("--out", default="results.jsonl")
    args = parser.parse_args()
    dirs = sorted(p.parent for p in SOURCE.glob("obj/*/*/program.py"))
    if args.limit:
        random.Random(args.seed).shuffle(dirs)
        dirs = dirs[: args.limit]
    OUT.mkdir(parents=True, exist_ok=True)
    results_path = OUT / args.out
    done = set()
    if results_path.exists():
        done = {json.loads(l)["id"] for l in results_path.open()}
    todo = [(d,) for d in dirs if d.name not in done]
    start = time.time()
    count = Counter()
    with results_path.open("a") as out, Pool(args.jobs, maxtasksperchild=50) as pool:
        for i, r in enumerate(pool.imap_unordered(process, todo, chunksize=1), 1):
            out.write(json.dumps(r) + "\n")
            out.flush()
            count[r["status"]] += 1
            if i % 100 == 0:
                print(f"{i}/{len(todo)} {time.time() - start:.0f}s {dict(count)}", flush=True)
    print("done", dict(count), flush=True)


if __name__ == "__main__":
    main()
