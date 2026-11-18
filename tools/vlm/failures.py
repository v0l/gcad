import argparse
import collections
import json
from pathlib import Path

BUCKETS = (
    "empty",
    "timeout",
    "crash",
    "syntax",
    "unknown_operation",
    "bad_argument",
    "bad_reference",
    "boolean_failure",
    "fillet_failure",
    "other",
)


def bucket(row):
    if row.get("built"):
        return None
    error = (row.get("error") or "").lower()
    if not row.get("program", "").strip():
        return "empty"
    if error == "timeout":
        return "timeout"
    if "backtrace" in error or "panicked" in error:
        return "crash"
    if "is not an operation name" in error or "unexpected" in error:
        return "syntax"
    if "unknown operation" in error:
        return "unknown_operation"
    if "matched no edges" in error or "no such" in error or "unknown body" in error or "not defined" in error:
        return "bad_reference"
    if "needs `" in error or "needs a" in error or "expected" in error or "cannot parse" in error:
        return "bad_argument"
    if "invalid output shell" in error or "cut failed" in error or "union failed" in error or "combine failed" in error:
        return "boolean_failure"
    if "chamfer" in error or "fillet" in error or "rounding a corner" in error or "edge too short" in error:
        return "fillet_failure"
    return "other"


def read_rows(path):
    return [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]


def count(rows):
    counts = collections.Counter(b for b in map(bucket, rows) if b)
    return {name: counts[name] for name in BUCKETS if counts[name]}


def table(named_rows):
    names = [name for name, _ in named_rows]
    counted = [count(rows) for _, rows in named_rows]
    header = "| bucket | " + " | ".join(names) + " |"
    sep = "|---|" + "---|" * len(names)
    lines = [header, sep]
    total = ["failures"]
    for c, (_, rows) in zip(counted, named_rows):
        total.append(f"{sum(c.values())} of {len(rows)}")
    for name in BUCKETS:
        if any(name in c for c in counted):
            lines.append(f"| {name} | " + " | ".join(str(c.get(name, 0)) for c in counted) + " |")
    lines.append("| " + " | ".join(total) + " |")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("runs", nargs="+", help="NAME=results.jsonl")
    args = parser.parse_args()
    named = []
    for spec in args.runs:
        name, path = spec.split("=", 1)
        named.append((name, read_rows(path)))
    print(table(named))


if __name__ == "__main__":
    main()
