import re
import subprocess
import tempfile
from pathlib import Path

from reward import BUILD_TIMEOUT, GCAD, memory_cap, strip_fences

LINE_RE = re.compile(r"line (\d+)")
STUB = "rect 10 10 at=0,0\nextrude 10\n"


def build_error(program, gcad=GCAD, timeout=BUILD_TIMEOUT):
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / "p.gcad"
        out = Path(tmp) / "p.stl"
        src.write_text(program)
        try:
            r = subprocess.run([gcad, "export", str(src), str(out)], capture_output=True, text=True, timeout=timeout, preexec_fn=memory_cap())
        except subprocess.TimeoutExpired:
            return "timeout"
        if r.returncode == 0 and out.exists():
            return None
        lines = (r.stdout + r.stderr).strip().splitlines()
        return "\n".join(l for l in lines if l.startswith("Error")) or (lines[-1] if lines else "failed")


def failing_line(error):
    m = LINE_RE.search(error or "")
    return int(m.group(1)) if m else None


def drop_line(program, number):
    lines = program.splitlines()
    if not 1 <= number <= len(lines):
        return None
    return "\n".join(lines[: number - 1] + lines[number:]) + "\n"


def drop_failing_lines(program, build=build_error, max_drops=20):
    drops = 0
    error = build(program)
    while error and drops < max_drops:
        number = failing_line(error)
        if number is None:
            number = len(program.splitlines())
        shorter = drop_line(program, number)
        if shorter is None or not shorter.strip():
            break
        program = shorter
        drops += 1
        error = build(program)
    return program, error, drops


def repair(first, resample, k, build=build_error, max_drops=20):
    program = strip_fences(first)
    error = build(program)
    if error is None:
        return {"program": program, "built": True, "tries": 1, "stage": "single_shot", "drops": 0}
    tries = 1
    for _ in range(k - 1):
        tries += 1
        candidate = strip_fences(resample())
        if build(candidate) is None:
            return {"program": candidate, "built": True, "tries": tries, "stage": "resample", "drops": 0}
    repaired, error, drops = drop_failing_lines(program, build, max_drops)
    if error is None:
        return {"program": repaired, "built": True, "tries": tries + 1, "stage": "drop_lines", "drops": drops}
    if build(STUB) is None:
        return {"program": STUB, "built": True, "tries": tries + 1, "stage": "stub", "drops": drops}
    return {"program": repaired, "built": False, "tries": tries + 1, "stage": "failed", "drops": drops}
