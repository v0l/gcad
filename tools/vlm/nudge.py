import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from repair import build_error, failing_line
from reward import strip_fences

NUMBER = re.compile(r"(?<![\w.])-?\d+(?:\.\d+)?")


def nudged(line, delta):
    head, _, rest = line.partition(" ")
    return head + " " + NUMBER.sub(lambda m: f"{float(m.group(0)) + delta:g}", rest)


def fixed_by_nudge(program, error, deltas=(0.01, -0.01)):
    number = failing_line(error)
    if number is None:
        return False
    lines = program.splitlines()
    if not 1 <= number <= len(lines):
        return False
    for delta in deltas:
        trial = lines[: number - 1] + [nudged(lines[number - 1], delta)] + lines[number:]
        if build_error("\n".join(trial) + "\n") is None:
            return True
    return False


def main():
    for path in sys.argv[1:]:
        rows = [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]
        failed = [r for r in rows if not r["built"]]
        fixed = sum(fixed_by_nudge(strip_fences(r["program"]), r["error"] or "") for r in failed)
        print(json.dumps({"file": path, "failed": len(failed), "fixed_by_nudge": fixed}), flush=True)


if __name__ == "__main__":
    main()
