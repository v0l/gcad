import io
import sys
from multiprocessing import Pool
import pyarrow.parquet as pq
from PIL import Image
import trimesh
from common import *

SOURCE = "zero-to-cad"
REPO = "ADSKAILab/Zero-To-CAD-100k"
COLS = ["uuid", "cadquery_file", "num_faces", "cadquery_ops_json", "cadquery_ops_count", "image_0", "stl_file", "step_file"]


def download():
    check_space(30, SOURCE)
    hf_snapshot(REPO, SOURCE, ["data/*", "README.md"])


def decode(v):
    return v.decode() if isinstance(v, bytes) else v


def process_file(args):
    pf, split = args
    od = out_dir(SOURCE)
    rows, dropped = [], []
    p = pq.ParquetFile(pf)
    for batch in p.iter_batches(batch_size=16, columns=COLS):
        for r in batch.to_pylist():
            uid = r["uuid"]
            d = od / "obj" / uid[:2] / uid
            glb = od / "glb" / uid[:2] / f"{uid}.glb"
            try:
                d.mkdir(parents=True, exist_ok=True)
                if not r["stl_file"]:
                    raise ValueError("no stl")
                stl = d / "model.stl"
                stl.write_bytes(r["stl_file"])
                if not r["step_file"]:
                    raise ValueError("no step")
                (d / "model.step").write_bytes(r["step_file"])
                (d / "program.py").write_text(decode(r["cadquery_file"]))
                (d / "ops.json").write_text(decode(r["cadquery_ops_json"]) or "[]")
                has_img = False
                if r["image_0"]:
                    Image.open(io.BytesIO(r["image_0"])).convert("RGB").save(d / "view0.png")
                    has_img = True
                glb.parent.mkdir(parents=True, exist_ok=True)
                m = trimesh.load(str(stl), force="mesh", process=False)
                trimesh.Scene(m).export(str(glb), file_type="glb")
                st = mesh_stats(glb)
                stl.unlink()
                row = row_base(SOURCE, uid, f"https://huggingface.co/datasets/{REPO}", "Apache-2.0", True,
                               "Zero-to-CAD 100K, Autodesk Research, Apache-2.0 (synthetic CadQuery programs)", "cad")
                row["paths"] = {"glb": str(glb.relative_to(DATA)), "step": str((d / "model.step").relative_to(DATA)),
                                "build_history": str((d / "program.py").relative_to(DATA)),
                                "ops_json": str((d / "ops.json").relative_to(DATA)),
                                "render": str((d / "view0.png").relative_to(DATA)) if has_img else None}
                row["build_history_format"] = "cadquery-python"
                row["has_build_history"] = True
                row["split"] = split
                row["ops_count"] = r["cadquery_ops_count"]
                row["brep_faces"] = int(r["num_faces"]) if r["num_faces"] is not None else None
                row.update({"triangles": st["triangles"], "watertight": st["watertight"], "parts": st["parts"],
                            "bbox_min": st["bbox_min"], "bbox_max": st["bbox_max"]})
                row.update(fit_label(st, "cad", True))
                rows.append(row)
            except Exception as e:
                dropped.append({"id": f"{SOURCE}:{uid}", "source": SOURCE, "reason": f"{type(e).__name__}: {str(e)[:200]}"})
                shutil.rmtree(d, ignore_errors=True)
    return rows, dropped


def normalise(limit=None):
    files = []
    for split in ("train", "val", "test"):
        files += [(f, split) for f in sorted((raw_dir(SOURCE) / "data" / split).glob("*.parquet"))]
    if limit:
        files = files[: int(limit)]
    rows, dropped = [], []
    with Pool(8) as pool:
        for i, (r, d) in enumerate(pool.imap_unordered(process_file, files)):
            rows += r
            dropped += d
            print(i + 1, len(files), len(rows), len(dropped), flush=True)
    rows.sort(key=lambda r: r["id"])
    od = out_dir(SOURCE)
    write_jsonl(od / "manifest.jsonl", rows)
    write_jsonl(od / "dropped.jsonl", dropped)


if __name__ == "__main__":
    steps = sys.argv[1:] or ["download", "normalise"]
    for s in steps:
        name, _, arg = s.partition("=")
        globals()[name](*([arg] if arg else []))
