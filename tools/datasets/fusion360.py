import json
import shutil
import sys
import zipfile
from common import *

SOURCE = "fusion360"
URL = "https://fusion-360-gallery-dataset.s3.us-west-2.amazonaws.com/reconstruction/r1.0.1/r1.0.1.zip"


def download():
    check_space(8, SOURCE)
    rd = raw_dir(SOURCE)
    aria(URL, rd / "r1.0.1.zip")
    if not (rd / "r1.0.1").exists():
        with zipfile.ZipFile(rd / "r1.0.1.zip") as z:
            z.extractall(rd)


def normalise():
    rd = raw_dir(SOURCE)
    base = rd / "r1.0.1" / "reconstruction"
    jobs = []
    last = {}
    for p in sorted(base.glob("*.obj")):
        last[p.stem.rsplit("_", 1)[0]] = p
    for js in sorted(base.glob("*.json")):
        oid = js.stem
        obj = last.get(oid)
        if obj is None:
            continue
        row = row_base(SOURCE, oid, "https://github.com/AutodeskAILab/Fusion360GalleryDataset",
                       "LicenseRef-Fusion360-Gallery-NonCommercial-Research", False,
                       "Fusion 360 Gallery Dataset, Autodesk, Inc. Non-commercial research use only", "cad",
                       has_build_history=True)
        row["paths"] = {"build_history": str(js.relative_to(DATA))}
        step = obj.with_suffix(".step")
        if step.exists():
            row["paths"]["step"] = str(step.relative_to(DATA))
        row["build_history_format"] = "fusion360-json"
        jobs.append(dict(src=obj, out_glb=out_dir(SOURCE) / "glb" / f"{oid}.glb", category="cad", cad=True, row=row))
    normalise_all(SOURCE, jobs)


if __name__ == "__main__":
    for step in sys.argv[1:] or ["download", "normalise"]:
        globals()[step]()
