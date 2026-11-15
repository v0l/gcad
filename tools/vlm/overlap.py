import json
import re
from pathlib import Path

MINDS_EYE_DIR = Path(__file__).resolve().parent.parent.parent / "bench" / "minds-eye"
STOPWORDS = frozenset("a an the with and of on in for to its it is are two three four five that has have at by from".split())
SIMILARITY_LIMIT = 0.6


def words(text):
    return [w for w in re.findall(r"[a-z0-9]+", text.lower()) if w not in STOPWORDS]


def load_minds_eye(root=MINDS_EYE_DIR):
    tasks = []
    for path in sorted(Path(root, "tasks").glob("*.jsonl")):
        tasks.extend(json.loads(l) for l in path.read_text().splitlines() if l.strip())
    names = {t["id"] for t in tasks}
    names |= {p.stem.split("-")[0] for p in Path(root, "ceiling").glob("*.gcad")}
    return {"names": names, "prompts": [set(words(t["prompt"])) for t in tasks]}


def similarity(a, b):
    if not a or not b:
        return 0.0
    return len(a & b) / len(a | b)


def overlaps_minds_eye(brief, banned, limit=SIMILARITY_LIMIT):
    tokens = words(brief)
    if banned["names"] & set(tokens):
        return True
    bag = set(tokens)
    return any(similarity(bag, p) >= limit for p in banned["prompts"])


def inside_minds_eye(path, root=MINDS_EYE_DIR):
    try:
        Path(path).resolve().relative_to(Path(root).resolve())
        return True
    except ValueError:
        return False


def refuse_minds_eye(path, root=MINDS_EYE_DIR):
    if inside_minds_eye(path, root):
        raise ValueError(f"{path} is inside bench/minds-eye: minds-eye is held-out evaluation, never training data")
