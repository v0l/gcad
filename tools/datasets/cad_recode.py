import sys
import multiprocessing as mp
from common import *

SOURCE = "cad-recode"
REPO = "filapro/cad-recode-v1.5"
BATCHES = 1
EXECUTE = 3000
TIMEOUT = 30


def download():
    check_space(2, SOURCE)
    hf_snapshot(REPO, SOURCE, [f"train/batch_{i:02d}/*" for i in range(BATCHES)] + ["README.md"])


def run_program(args):
    py, glb = args
    import cadquery as cq
    import trimesh
    import tempfile
    ns = {}
    src = Path(py).read_text()
    exec(compile(src, str(py), "exec"), ns)
    res = ns.get("r")
    if res is None:
        raise ValueError("no result r")
    shape = res.val() if hasattr(res, "val") else res
    with tempfile.NamedTemporaryFile(suffix=".stl") as t:
        cq.exporters.export(res, t.name, exportType="STL", tolerance=0.01, angularTolerance=0.1)
        m = trimesh.load(t.name, force="mesh", process=False)
    Path(glb).parent.mkdir(parents=True, exist_ok=True)
    trimesh.Scene(m).export(str(glb), file_type="glb")
    return True


def guarded(args, q):
    try:
        run_program(args)
        q.put("ok")
    except Exception as e:
        q.put(f"{type(e).__name__}: {str(e)[:150]}")


def run_isolated(args):
    q = mp.Queue()
    p = mp.Process(target=guarded, args=(args, q))
    p.start()
    p.join(TIMEOUT)
    if p.is_alive():
        p.kill()
        p.join()
        return "timeout"
    return q.get() if not q.empty() else "crashed"


def job(py):
    uid = f"{py.parent.name}_{py.stem}"
    glb = out_dir(SOURCE) / "glb" / f"{uid}.glb"
    status = "ok" if glb.exists() else run_isolated((py, glb))
    if status != "ok":
        return ("drop", {"id": f"{SOURCE}:{uid}", "source": SOURCE, "src": str(py), "reason": status})
    try:
        st = mesh_stats(glb)
    except Exception as e:
        return ("drop", {"id": f"{SOURCE}:{uid}", "source": SOURCE, "src": str(py), "reason": f"{type(e).__name__}: {e}"})
    row = row_base(SOURCE, uid, f"https://huggingface.co/datasets/{REPO}", "CC-BY-NC-4.0", False,
                   "CAD-Recode v1.5 (Rukhovich et al.), CC BY-NC 4.0, research use only", "cad", has_build_history=True)
    row["paths"] = {"glb": str(glb.relative_to(DATA)), "build_history": str(py.relative_to(DATA))}
    row["build_history_format"] = "cadquery-python"
    row.update({"triangles": st["triangles"], "watertight": st["watertight"], "parts": st["parts"],
                "bbox_min": st["bbox_min"], "bbox_max": st["bbox_max"]})
    row.update(fit_label(st, "cad", True))
    return ("ok", row)


def normalise():
    files = []
    for i in range(BATCHES):
        files += sorted((raw_dir(SOURCE) / "train" / f"batch_{i:02d}").glob("*.py"))
    import random
    random.Random(548).shuffle(files)
    files = files[:EXECUTE]
    rows, dropped = [], []
    with ThreadPoolExecutor(8) as ex:
        for status, r in ex.map(job, files):
            (rows if status == "ok" else dropped).append(r)
    rows.sort(key=lambda r: r["id"])
    od = out_dir(SOURCE)
    write_jsonl(od / "manifest.jsonl", rows)
    write_jsonl(od / "dropped.jsonl", dropped)
    print(len(rows), "kept", len(dropped), "dropped")


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
