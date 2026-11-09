import json
import os
import subprocess

from PIL import Image

from agent_api import provider, chat, image_part
from metrics import gcad

JUDGE_VERSION = os.environ.get("JUDGE_VERSION", "v2")
ANCHORS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "anchors")
JUDGE_MODEL = os.environ.get("JUDGE_MODEL", "code-x-high")
JUDGE_PROVIDER = os.environ.get("JUDGE_PROVIDER", "yalr")

SCORE_PROMPT = """You judge 3D models made by an AI agent for a game or CAD request. Ignore background and lighting.
Request: "{prompt}"
Rubric items:
{rubric}

The first image shows four views of the model (iso, top, front, right). The second shows six views turning round the model.
For each rubric item say whether it is present and recognisable (true or false). Then give an overall score from 1 to 10 for how good a game asset or part this is: 1 is nothing recognisable, 3 is a pile of primitives, 5 is a recognisable but crude blockout, 7 is a good clean asset, 10 is professional. Be strict and use the whole range.
Reply with JSON only: {{"items": [true or false for each rubric item in order], "overall": n, "why": "one sentence"}}"""

ANCHOR_PROMPT = """You judge 3D models made by an AI agent for a game or CAD request. Ignore background and lighting.
The first three images are fixed reference renders that a person rated, and they define the scale: image 1 is a 2 out of 10 (a smooth blob figure: recognisable, but it looks like clay lumps, with no crisp form, no real detail and no finish), image 2 is a 5 (a clean CAD solid with every part present and tidy, but plain), image 3 is an 8 (a precise, finished part with fillets, holes and clean detail). Organic and soft blobby models are held to the same standard: only models that look as finished as the 8 score 8, and a model that looks like the 2 scores 2 or lower, whatever the request. Use the whole range and do not drift above the anchors.
Request: "{prompt}"
Rubric items:
{rubric}

The fourth image shows four views of the model to score (iso, top, front, right). The fifth shows six views turning round it.
For each rubric item say whether it is present and recognisable (true or false). Then give an overall score from 1 to 10 for how good a game asset or part this is, on the scale of the three references.
Reply with JSON only: {{"items": [true or false for each rubric item in order], "overall": n, "why": "one sentence"}}"""

PAIR_PROMPT = """Two 3D models were made for the same request: "{prompt}"
Rubric:
{rubric}

Images 1 and 2 are model A (four views, then six views turning round). Images 3 and 4 are model B in the same way.
Judge which is the better game asset or part: rubric coverage, recognisable anatomy or function, proportions, detail and finish. Ignore background and lighting.
Reply with JSON only: {{"winner": "A" or "B" or "tie", "why": "one sentence"}}"""


def render_views(gcad_path, outdir):
    sheet = os.path.join(outdir, "views4.png")
    turns = os.path.join(outdir, "views6.png")
    ok = gcad("render", gcad_path, sheet, timeout=300).returncode == 0
    ok = gcad("render", gcad_path, turns, "--turns", "6", timeout=300).returncode == 0 and ok
    return (sheet, turns) if ok and os.path.exists(sheet) and os.path.exists(turns) else None


def numbered(rubric):
    return "\n".join(f"{i + 1}. {r}" for i, r in enumerate(rubric))


def ask(content):
    api = provider(JUDGE_PROVIDER)
    resp = chat(api, {"model": JUDGE_MODEL, "max_tokens": 2000, "temperature": 0,
                      "messages": [{"role": "user", "content": content}]})
    text = resp["choices"][0]["message"]["content"]
    return json.loads(text[text.index("{"): text.rindex("}") + 1])


def anchor_images():
    return [os.path.join(ANCHORS, f"score{n}.png") for n in (2, 5, 8)]


def score(task, images, anchors=None):
    if anchors is None:
        anchors = JUDGE_VERSION != "v1"
    if anchors:
        prompt = ANCHOR_PROMPT.format(prompt=task["prompt"], rubric=numbered(task["rubric"]))
        refs = [image_part(p, 800) for p in anchor_images()]
    else:
        prompt = SCORE_PROMPT.format(prompt=task["prompt"], rubric=numbered(task["rubric"]))
        refs = []
    j = ask(refs + [image_part(images[0], 1280), image_part(images[1], 1600), {"type": "text", "text": prompt}])
    items = [bool(x) for x in j["items"]][: len(task["rubric"])]
    items += [False] * (len(task["rubric"]) - len(items))
    return {"items": items, "rubric_fraction": round(sum(items) / len(items), 3),
            "overall": float(j["overall"]), "why": j.get("why", ""), "judge": f"{JUDGE_VERSION}:{JUDGE_MODEL}"}


def pair(task, images_a, images_b):
    prompt = PAIR_PROMPT.format(prompt=task["prompt"], rubric=numbered(task["rubric"]))
    content = [image_part(p, 1280) for p in (*images_a, *images_b)] + [{"type": "text", "text": prompt}]
    return ask(content)


def blind_pair(task, images_x, images_y, flip):
    a, b = (images_y, images_x) if flip else (images_x, images_y)
    j = pair(task, a, b)
    w = j.get("winner")
    if w == "tie":
        return {"winner": "tie", "why": j.get("why", "")}
    first_is_x = not flip
    x_won = (w == "A") == first_is_x
    return {"winner": "x" if x_won else "y", "why": j.get("why", "")}


def contact_sheet(images, out, cols=4, cell=420):
    rows = (len(images) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * cell, rows * cell), "white")
    for i, p in enumerate(images):
        im = Image.open(p).convert("RGB")
        im.thumbnail((cell, cell))
        sheet.paste(im, ((i % cols) * cell, (i // cols) * cell))
    sheet.save(out)
