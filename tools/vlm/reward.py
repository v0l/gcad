import os
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "datasets"))

from shape_distance import compare_meshes, load_mesh

GCAD = os.environ.get("GCAD_BIN", "gcad")
CHAMFER_SCALE = 0.05
BUILD_TIMEOUT = 40
SAMPLES = 100000
NOISE_FLOOR = 0.002


def strip_fences(text):
    text = text.strip()
    if text.startswith("```"):
        lines = text.splitlines()[1:]
        if lines and lines[-1].strip().startswith("```"):
            lines = lines[:-1]
        text = "\n".join(lines)
    return text.strip() + "\n"


def export_stl(program, workdir):
    src = Path(workdir) / "pred.gcad"
    stl = Path(workdir) / "pred.stl"
    src.write_text(program)
    try:
        r = subprocess.run([GCAD, "export", str(src), str(stl)], capture_output=True, text=True, timeout=BUILD_TIMEOUT)
    except subprocess.TimeoutExpired:
        return None, "timeout"
    if r.returncode != 0 or not stl.exists():
        message = (r.stdout + r.stderr).strip().splitlines()
        return None, (message[-1] if message else "failed")[:120]
    return stl, None


def reward_from_metrics(metrics):
    if metrics["volume_a"] <= 0 or metrics["volume_b"] <= 0:
        return 0.0
    shape = max(0.0, 1.0 - max(0.0, metrics["chamfer"] - NOISE_FLOOR) / CHAMFER_SCALE)
    volume = max(0.0, 1.0 - metrics["volume_error"])
    return 0.1 + 0.9 * shape * volume


def score(program, reference_mesh):
    program = strip_fences(program)
    result = {"built": False, "reward": 0.0, "error": None}
    with tempfile.TemporaryDirectory() as tmp:
        stl, error = export_stl(program, tmp)
        if stl is None:
            result["error"] = error
            return result
        try:
            built = load_mesh(stl)
            metrics = compare_meshes(load_mesh(reference_mesh), built, samples=SAMPLES)
        except Exception as e:
            result["error"] = "compare:" + str(e)[:100]
            return result
    result["built"] = True
    result["metrics"] = metrics
    result["reward"] = reward_from_metrics(metrics)
    return result
