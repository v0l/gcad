import argparse
import json
import re
from pathlib import Path


def result_line(summary, generated, kept, dropped):
    train, val, test = (summary[s]["concept"] for s in ("train", "val", "test"))
    clean = sum(summary[s]["clean"] for s in summary)
    return (
        f"Result: {generated} concept images generated for pairs, {kept} kept by the silhouette filter and {dropped} dropped. "
        f"Concept pairs: train {train}, val {val}, test {test} ({train + val + test} in total), mixed with {clean} clean four-view examples."
    )


def replace_result(doc_text, line):
    return re.sub(r"^Result: .*$", lambda _: line, doc_text, count=1, flags=re.M)


def count_lines(path):
    p = Path(path)
    return sum(1 for l in p.read_text().splitlines() if l.strip()) if p.exists() else 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--summary", required=True)
    ap.add_argument("--generated", required=True)
    ap.add_argument("--kept", required=True)
    ap.add_argument("--dropped", required=True)
    ap.add_argument("--doc", required=True)
    args = ap.parse_args()
    line = result_line(json.loads(Path(args.summary).read_text()), count_lines(args.generated), count_lines(args.kept), count_lines(args.dropped))
    doc = Path(args.doc)
    doc.write_text(replace_result(doc.read_text(), line))
    print(line)


if __name__ == "__main__":
    main()
