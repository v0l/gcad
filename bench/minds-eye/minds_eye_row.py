import argparse
import json
from pathlib import Path

HEADER = "| config | runs | built | judge v2 | rubric | hard | tokens/run | seconds | pairwise |"


def kilo(n):
    return f"{round(n / 1000)}k" if n is not None else ""


def fmt(x, digits):
    return "" if x is None else f"{x:.{digits}f}"


def format_row(label, summary, pairwise="not run"):
    built = summary["build_rate"]
    return (
        f"| {label} | {summary['runs']} | {'' if built is None else f'{built * 100:.0f}%'} | {fmt(summary['judge_overall'], 1)} | "
        f"{fmt(summary['rubric_fraction'], 2)} | {fmt(summary['hard_score'], 2)} | {kilo(summary['tokens'])} | "
        f"{'' if summary['seconds'] is None else round(summary['seconds'])} | {pairwise} |"
    )


def insert_row(doc_text, row):
    lines = doc_text.splitlines()
    label = row.split("|")[1].strip()
    lines = [line for line in lines if not line.startswith(f"| {label} |")]
    start = lines.index(HEADER)
    end = start + 2
    while end < len(lines) and lines[end].startswith("|"):
        end += 1
    lines.insert(end, row)
    return "\n".join(lines) + "\n"


def pairwise_cell(path, against):
    if not path:
        return "not run"
    p = json.loads(Path(path).read_text())
    ties = f", {p['ties']} ties" if p["ties"] else ""
    return f"{p['wins_a']}-{p['wins_b']} vs {against}{ties}"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--result", required=True)
    ap.add_argument("--doc", required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--pairwise", default="")
    ap.add_argument("--against", default="local")
    args = ap.parse_args()
    summary = json.loads(Path(args.result).read_text())["summary"]
    row = format_row(args.label, summary, pairwise_cell(args.pairwise, args.against))
    doc = Path(args.doc)
    doc.write_text(insert_row(doc.read_text(), row))
    print(row)


if __name__ == "__main__":
    main()
