import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from repair import build_error, failing_line
from reward import strip_fences

CUT = re.compile(r"mode=cut|^hole |^combine .*mode=cut")


def cut_misses_solid(program, error):
    number = failing_line(error)
    if number is None or "no boundary shells" not in error:
        return False
    lines = program.splitlines()
    if not CUT.search(lines[number - 1]):
        return False
    return build_error("\n".join(lines[: number - 1] + lines[number:]) + "\n") is None


def main():
    for path in sys.argv[1:]:
        rows = [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]
        failed = [r for r in rows if not r["built"]]
        misses = sum(cut_misses_solid(strip_fences(r["program"]), r["error"] or "") for r in failed)
        print(json.dumps({"file": path, "failed": len(failed), "cut_in_air_fixed_by_drop": misses}))


if __name__ == "__main__":
    main()
