import json
import os
from pathlib import Path

import part_dims

DATA = Path(os.environ.get("GCAD_DATA", "/home/kieran/gcad-datasets"))
PAIRS = DATA / "out" / "pairs" / "pairs.jsonl"
VIEWS = ("iso", "front", "top", "right")
PROMPT = (
    "The images show one part from four views: iso, front, top and right. "
    "Write the .gcad program, one operation per line, that builds it. "
    "Reply with the program only."
)
DIMS_PROMPT = (
    "The images show one part from four views: iso, front, top and right. "
    "Write the .gcad program, one operation per line, that builds it. "
    "Dimensions in mm: {spec} "
    "Reply with the program only."
)
DIMS_P = 0.3
DIMS_SEED = 1


def prompt_for(program, key, dims=False, p=DIMS_P, seed=DIMS_SEED):
    if not dims:
        return PROMPT
    spec = part_dims.describe(program, key, p=p, seed=seed)
    if not spec:
        return PROMPT
    return DIMS_PROMPT.format(spec=spec.strip())


def read_pairs(path=PAIRS, split=None, commercial_only=True, max_lines=None):
    rows = []
    for line in Path(path).read_text().splitlines():
        row = json.loads(line)
        if split and row["split"] != split:
            continue
        if commercial_only and not row["commercial_ok"]:
            continue
        if max_lines and row["lines"] > max_lines:
            continue
        rows.append(row)
    return rows


def row_prompt(row, data=DATA, dims=False, p=DIMS_P, seed=DIMS_SEED):
    if not dims:
        return PROMPT
    return prompt_for((data / row["gcad"]).read_text(), row["id"], dims=True, p=p, seed=seed)


def chat_example(row, data=DATA, views=VIEWS, dims=False, p=DIMS_P, seed=DIMS_SEED):
    images = [str(data / row["renders"][v]) for v in views]
    program = (data / row["gcad"]).read_text().strip() + "\n"
    content = [{"type": "image", "image": path} for path in images]
    content.append({"type": "text", "text": row_prompt(row, data, dims, p, seed)})
    return {
        "id": row["id"],
        "messages": [
            {"role": "user", "content": content},
            {"role": "assistant", "content": [{"type": "text", "text": program}]},
        ],
    }


def op_histogram(rows):
    counts = {}
    for row in rows:
        for op in row["ops"]:
            counts[op] = counts.get(op, 0) + 1
    return dict(sorted(counts.items(), key=lambda kv: -kv[1]))
