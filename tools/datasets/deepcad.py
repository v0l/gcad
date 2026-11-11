import sys
import tarfile
from multiprocessing import Pool
from common import *

SOURCE = "deepcad"
URL = "http://www.cs.columbia.edu/cg/deepcad/data.tar"
CAP = 20000


def download():
    check_space(6, SOURCE)
    rd = raw_dir(SOURCE)
    aria(URL, rd / "data.tar")
    if not (rd / "data" / "cad_json").exists():
        with tarfile.open(rd / "data.tar") as t:
            t.extractall(rd)
        with tarfile.open(rd / "data" / "cad_json.tar.gz") as t:
            t.extractall(rd / "data")


def summarise(path):
    try:
        j = json.load(open(path))
        ents = j["entities"]
        seq = j.get("sequence", [])
        kinds = [e["type"] for e in ents.values()]
        if "ExtrudeFeature" not in kinds:
            return None
        return {"path": str(Path(path).relative_to(DATA)), "sketches": kinds.count("Sketch"),
                "extrudes": kinds.count("ExtrudeFeature"), "steps": len(seq)}
    except Exception:
        return None


def normalise():
    rd = raw_dir(SOURCE) / "data"
    split = json.load(open(rd / "train_val_test_split.json"))
    sp = {}
    for k, ids in split.items():
        for i in ids:
            sp[i] = k
    files = sorted((rd / "cad_json").rglob("*.json"))
    import random
    random.Random(548).shuffle(files)
    files = files[: CAP * 2]
    od = out_dir(SOURCE)
    rows, dropped = [], []
    with Pool(8) as pool:
        for f, s in zip(files, pool.imap(summarise, files, chunksize=64)):
            oid = f.stem
            if s is None:
                dropped.append({"id": f"{SOURCE}:{oid}", "source": SOURCE, "reason": "no extrude feature or unreadable json"})
                continue
            if len(rows) >= CAP:
                break
            row = row_base(SOURCE, oid, "https://github.com/ChrisWu1997/DeepCAD", "LicenseRef-Onshape-ToU-1.g.ii", False,
                           "DeepCAD (Wu et al., ICCV 2021), models from Onshape public documents; copyright stays with their creators",
                           "cad", has_build_history=True)
            row["paths"] = {"build_history": s["path"]}
            row["build_history_format"] = "deepcad-json"
            row["split"] = sp.get(f"{f.parent.name}/{oid}", "unknown")
            row.update({"sketches": s["sketches"], "extrudes": s["extrudes"], "triangles": None, "watertight": None,
                        "gcad_fit": "high", "gcad_fit_score": 0.75, "shape_kind": "mechanical"})
            rows.append(row)
    rows.sort(key=lambda r: r["id"])
    write_jsonl(od / "manifest.jsonl", rows)
    write_jsonl(od / "dropped.jsonl", dropped)
    print(len(rows), "kept", len(dropped), "dropped")


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
