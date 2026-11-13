import argparse
import json
import random
from pathlib import Path

from pairs_data import DATA, PROMPT, VIEWS, chat_example, read_pairs

CONCEPT_PROMPT = (
    "The image is a concept picture of one part. "
    "Write the .gcad program, one operation per line, that builds it. "
    "Reply with the program only."
)


def load_concepts(manifest, kept):
    concepts = {}
    for line in Path(manifest).read_text().splitlines():
        m = json.loads(line)
        if m.get("has_concept") and m.get("paths", {}).get("concept"):
            path = m["paths"]["concept"][0] if isinstance(m["paths"]["concept"], list) else m["paths"]["concept"]
            concepts[m["id"]] = (str(DATA / path) if not str(path).startswith("/") else str(path), m.get("concept_kind") or "real")
    for line in Path(kept).read_text().splitlines():
        k = json.loads(line)
        concepts.setdefault(k["id"], (k["concept"], "synthetic"))
    return concepts


def concept_example(row, concept_path, kind):
    program = (DATA / row["gcad"]).read_text().strip() + "\n"
    content = [{"type": "image", "image": concept_path}, {"type": "text", "text": CONCEPT_PROMPT}]
    return {
        "id": row["id"],
        "concept_kind": kind,
        "messages": [
            {"role": "user", "content": content},
            {"role": "assistant", "content": [{"type": "text", "text": program}]},
        ],
    }


def build(rows, concepts, clean_per_concept, seed):
    rng = random.Random(seed)
    with_concept = [r for r in rows if r["id"] in concepts and Path(concepts[r["id"]][0]).exists()]
    examples = [concept_example(r, *concepts[r["id"]]) for r in with_concept]
    others = [r for r in rows if r["id"] not in {w["id"] for w in with_concept}]
    rng.shuffle(others)
    clean = [chat_example(r) for r in others[: clean_per_concept * max(1, len(examples))]]
    return examples, clean


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", default=str(DATA / "out" / "manifest.synthetic.jsonl"))
    parser.add_argument("--kept", default=str(DATA / "out" / "synthetic" / "kept_img2img.jsonl"))
    parser.add_argument("--out", default=str(DATA / "out" / "vlm_concept"))
    parser.add_argument("--max-lines", type=int, default=60)
    parser.add_argument("--clean-per-concept", type=int, default=20)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    concepts = load_concepts(args.manifest, args.kept)
    summary = {}
    for split in ("train", "val", "test"):
        rows = read_pairs(split=split, max_lines=args.max_lines)
        examples, clean = build(rows, concepts, args.clean_per_concept, args.seed)
        mixed = examples + clean
        random.Random(args.seed).shuffle(mixed)
        with open(out / f"{split}.jsonl", "w") as f:
            for e in mixed:
                f.write(json.dumps(e) + "\n")
        summary[split] = {"concept": len(examples), "clean": len(clean)}
    (out / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
