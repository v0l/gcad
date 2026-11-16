import argparse
import json
import os
import re
import sys
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import overlap
from pairs_data import DATA, read_pairs

LOCAL_URL = os.environ.get("BRIEF_URL", "http://127.0.0.1:8001/v1")
LOCAL_MODEL = os.environ.get("BRIEF_MODEL", "orcarouter/Qwen3.8-27B-Uncensored-NVFP4")
CAPTION_PROMPT = (
    "Below is a gcad program, one CAD operation per line. Write the request a person would type to ask for "
    "this part, as one or two plain sentences. Name what the part is or looks like and give its main sizes "
    "in mm. Do not mention operations, gcad or line numbers. Reply with the request only.\n\n{program}"
)
BRIEF_STYLES = (
    "{brief}",
    "Make me {brief}",
    "Model this: {brief}",
    "I need a part. {brief}",
    "Request: {brief}",
)


def clean_brief(text):
    text = re.sub(r"<think>.*?</think>", "", text, flags=re.S).strip()
    text = text.strip("\"' \n")
    if not text or "```" in text or len(text) > 400:
        return None
    return " ".join(text.split())


def caption(program, ask, banned=None):
    brief = clean_brief(ask(CAPTION_PROMPT.format(program=program.strip())))
    if brief and banned and overlap.overlaps_minds_eye(brief, banned):
        return None
    return brief


def ask_local(prompt):
    body = {"model": LOCAL_MODEL, "max_tokens": 200, "temperature": 0.3,
            "messages": [{"role": "user", "content": prompt}],
            "chat_template_kwargs": {"enable_thinking": False}}
    req = urllib.request.Request(LOCAL_URL.rstrip("/") + "/chat/completions", json.dumps(body).encode(),
                                 {"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))["choices"][0]["message"]["content"] or ""


def styled(brief, key):
    return BRIEF_STYLES[sum(map(ord, key)) % len(BRIEF_STYLES)].format(brief=brief)


def text_example(row_id, brief, program):
    return {
        "id": row_id,
        "messages": [
            {"role": "user", "content": [{"type": "text", "text": styled(brief, row_id)}]},
            {"role": "assistant", "content": [{"type": "text", "text": program.strip() + "\n"}]},
        ],
    }


def caption_rows(rows, data, ask, workers=4, done=None, banned=None):
    done = done or {}

    def one(row):
        if row["id"] in done:
            return row["id"], done[row["id"]]
        return row["id"], caption((data / row["gcad"]).read_text(), ask, banned)

    with ThreadPoolExecutor(workers) as pool:
        return dict(pool.map(one, rows))


def load_done(path):
    if not Path(path).exists():
        return {}
    return {j["id"]: j["brief"] for j in map(json.loads, Path(path).read_text().splitlines()) if j.get("brief")}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default=str(DATA / "out" / "vlm_brief"))
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--max-lines", type=int, default=60)
    parser.add_argument("--workers", type=int, default=4)
    args = parser.parse_args()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    briefs_path = out / "briefs.jsonl"
    done = load_done(briefs_path)
    banned = overlap.load_minds_eye()
    done = {k: v for k, v in done.items() if not overlap.overlaps_minds_eye(v, banned)}
    splits = {}
    for split in ("train", "val", "test"):
        rows = read_pairs(split=split, max_lines=args.max_lines)
        splits[split] = rows[: args.limit] if args.limit else rows
    with open(briefs_path, "a") as f:
        for split, rows in splits.items():
            todo = [r for r in rows if r["id"] not in done]
            for start in range(0, len(todo), 64):
                chunk = todo[start:start + 64]
                got = caption_rows(chunk, DATA, ask_local, args.workers, banned=banned)
                for rid, brief in got.items():
                    if brief and not overlap.overlaps_minds_eye(brief, banned):
                        done[rid] = brief
                        f.write(json.dumps({"id": rid, "brief": brief}) + "\n")
                f.flush()
    for split, rows in splits.items():
        n = 0
        with open(out / f"{split}.jsonl", "w") as f:
            for row in rows:
                if row["id"] in done:
                    program = (DATA / row["gcad"]).read_text()
                    f.write(json.dumps(text_example(row["id"], done[row["id"]], program)) + "\n")
                    n += 1
        print(split, n, "of", len(rows))


if __name__ == "__main__":
    main()
