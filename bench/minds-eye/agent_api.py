import base64
import json
import os
import subprocess
import time
import urllib.request

from metrics import silhouettes, iou, load_obj, export_obj

PROVIDERS = os.path.expanduser("~/.pi/agent/models.json")


def provider(name="yalr"):
    return json.load(open(PROVIDERS))["providers"][name]


def chat(cfg, body, retries=3):
    req = urllib.request.Request(
        cfg["baseUrl"].rstrip("/") + "/chat/completions",
        json.dumps(body).encode(),
        {"Content-Type": "application/json", "Authorization": "Bearer " + cfg["apiKey"]},
    )
    err = None
    for _ in range(retries):
        try:
            return json.load(urllib.request.urlopen(req, timeout=600))
        except Exception as e:
            err = e
            time.sleep(3)
    raise err


def image_part(path, side=1024):
    from PIL import Image

    im = Image.open(path).convert("RGB")
    im.thumbnail((side, side))
    tmp = path + ".jpg"
    im.save(tmp, quality=85)
    data = base64.b64encode(open(tmp, "rb").read()).decode()
    os.remove(tmp)
    return {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64," + data}}


def foreground(path, side=256):
    import numpy as np
    from PIL import Image

    im = Image.open(path).convert("RGB").resize((side, side))
    a = np.asarray(im).astype(float)
    bg = np.median(np.concatenate([a[:4].reshape(-1, 3), a[-4:].reshape(-1, 3), a[:, :4].reshape(-1, 3), a[:, -4:].reshape(-1, 3)]), axis=0)
    return np.abs(a - bg).sum(axis=2) > 60


def silhouette_overlap(render_png, concept_png):
    a, b = foreground(render_png), foreground(concept_png)
    ys, xs = np_bbox(a)
    yb, xb = np_bbox(b)
    return {"iou": round(iou(fit(a, ys, xs), fit(b, yb, xb)), 3)}


def np_bbox(mask):
    import numpy as np

    ys, xs = np.where(mask)
    if len(ys) == 0:
        return (0, 1), (0, 1)
    return (ys.min(), ys.max() + 1), (xs.min(), xs.max() + 1)


def fit(mask, ys, xs, side=128):
    import numpy as np
    from PIL import Image

    crop = Image.fromarray((mask[ys[0]:ys[1], xs[0]:xs[1]] * 255).astype("uint8"))
    w, h = crop.size
    s = (side - 4) / max(w, h)
    crop = crop.resize((max(int(w * s), 1), max(int(h * s), 1)))
    canvas = Image.new("L", (side, side), 0)
    canvas.paste(crop, ((side - crop.size[0]) // 2, (side - crop.size[1]) // 2))
    return np.asarray(canvas) > 127


class Sandbox:
    def __init__(self, workdir, render, reference):
        self.dir = workdir
        self.render = render
        self.reference = reference
        self.calls = 0
        self.renders = 0
        self.pending_images = []

    def path(self, rel):
        full = os.path.realpath(os.path.join(self.dir, rel))
        if not full.startswith(os.path.realpath(self.dir)):
            raise ValueError("path outside the work directory")
        return full

    def write_file(self, path, content):
        full = self.path(path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        open(full, "w").write(content)
        return f"wrote {path} ({len(content.splitlines())} lines)"

    def read_file(self, path, offset=1, limit=400):
        lines = open(self.path(path)).read().splitlines()
        chunk = lines[offset - 1:offset - 1 + limit]
        return "\n".join(f"{offset + i}\t{ln}" for i, ln in enumerate(chunk)) or "(empty)"

    def gcad(self, args):
        if not self.render and args and args[0] in ("render", "inspect"):
            return "render and inspect are not available in this configuration"
        if args and args[0] in ("view", "sim", "remote"):
            return f"gcad {args[0]} is not available here"
        try:
            p = subprocess.run(["gcad", *args], cwd=self.dir, capture_output=True, text=True, timeout=300)
        except subprocess.TimeoutExpired:
            return "timed out after 300 s"
        out = (p.stdout + p.stderr).strip()
        if args and args[0] in ("render", "inspect") and p.returncode == 0:
            self.renders += 1
        return out[-6000:] if len(out) > 6000 else out

    def show_image(self, path):
        if not self.render:
            return "viewing images is not available in this configuration"
        full = self.path(path)
        self.pending_images.append(image_part(full))
        return f"image {path} is attached in the next message"

    def compare(self, render_png, concept_png="reference.png"):
        if not self.reference:
            return "compare is not available in this configuration"
        return json.dumps(silhouette_overlap(self.path(render_png), self.path(concept_png)))


def tool_specs(render, reference):
    def fn(name, desc, props, req):
        return {"type": "function", "function": {"name": name, "description": desc,
                                                  "parameters": {"type": "object", "properties": props, "required": req}}}

    s = {"type": "string"}
    specs = [
        fn("write_file", "Create or replace a file in the work directory", {"path": s, "content": s}, ["path", "content"]),
        fn("read_file", "Read a file in the work directory with line numbers",
           {"path": s, "offset": {"type": "integer"}, "limit": {"type": "integer"}}, ["path"]),
        fn("gcad", "Run the gcad CLI in the work directory, args as a list, like [\"check\", \"model.gcad\"]",
           {"args": {"type": "array", "items": s}}, ["args"]),
    ]
    if render:
        specs.append(fn("show_image", "Look at a PNG in the work directory, such as one written by gcad render",
                        {"path": s}, ["path"]))
    if reference:
        specs.append(fn("compare", "Silhouette overlap (IoU 0-1) between a render PNG and reference.png, both cropped and scaled to fit",
                        {"render_png": s}, ["render_png"]))
    return specs


def dispatch(box, name, args):
    box.calls += 1
    try:
        if name == "write_file":
            return box.write_file(args["path"], args["content"])
        if name == "read_file":
            return box.read_file(args["path"], args.get("offset", 1), args.get("limit", 400))
        if name == "gcad":
            return box.gcad([str(a) for a in args["args"]])
        if name == "show_image":
            return box.show_image(args["path"])
        if name == "compare":
            return box.compare(args["render_png"])
        return f"unknown tool {name}"
    except Exception as e:
        return f"error: {e}"


def run_api(cfg, prompt, system, workdir, budget, log):
    api = provider(cfg.get("provider", "yalr"))
    box = Sandbox(workdir, cfg.get("render", False), cfg.get("reference", False))
    messages = [{"role": "system", "content": system}, {"role": "user", "content": prompt}]
    tools = tool_specs(box.render, box.reference)
    started = time.time()
    usage = {"input": 0, "output": 0, "cached": 0, "requests": 0}
    stop = "finished"
    while True:
        if time.time() - started > budget["seconds"]:
            stop = "time budget"
            break
        if usage["input"] + usage["output"] > budget["tokens"]:
            stop = "token budget"
            break
        if usage["requests"] >= budget["turns"]:
            stop = "turn budget"
            break
        resp = chat(api, {"model": cfg["model"], "messages": messages, "tools": tools, "max_tokens": 16000})
        u = resp.get("usage") or {}
        usage["requests"] += 1
        usage["input"] += u.get("prompt_tokens", 0)
        usage["output"] += u.get("completion_tokens", 0)
        usage["cached"] += ((u.get("prompt_tokens_details") or {}).get("cached_tokens") or 0)
        msg = resp["choices"][0]["message"]
        messages.append({k: v for k, v in msg.items() if k in ("role", "content", "tool_calls") and v is not None})
        log.write(json.dumps({"t": round(time.time() - started, 1), "assistant": msg.get("content"),
                              "calls": [(c["function"]["name"], c["function"]["arguments"][:300]) for c in msg.get("tool_calls") or []]}) + "\n")
        log.flush()
        calls = msg.get("tool_calls") or []
        if not calls:
            break
        for c in calls:
            try:
                a = json.loads(c["function"]["arguments"] or "{}")
            except json.JSONDecodeError:
                a = {}
            out = dispatch(box, c["function"]["name"], a)
            messages.append({"role": "tool", "tool_call_id": c["id"], "content": out})
        if box.pending_images:
            messages.append({"role": "user", "content": box.pending_images + [{"type": "text", "text": "Here is the image."}]})
            box.pending_images = []
    return {"stop": stop, "seconds": round(time.time() - started, 1), "tokens": usage,
            "tool_calls": box.calls, "renders": box.renders}
