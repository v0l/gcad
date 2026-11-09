import json
import os
import subprocess
import tempfile

from agent_api import provider, chat, image_part

HERE = os.path.dirname(os.path.abspath(__file__))
FLUX = os.path.expanduser("~/git/kids-free-games/art/gen/flux.py")
CACHE = os.path.join(HERE, "results", "concepts")

STYLE = ("{prompt} Single object centred on a plain light grey background, full object in frame, "
         "three-quarter view, clean 3D game asset concept render, flat even lighting, no text.")


def concept(task, seed):
    os.makedirs(CACHE, exist_ok=True)
    out = os.path.join(CACHE, f"{task['id']}-{seed}.png")
    if os.path.exists(out):
        return out
    remote = f"/tmp/me-concept/{task['id']}-{seed}"
    job = [{"prompt": STYLE.format(prompt=task["prompt"]), "seed": 1000 + seed, "out": remote + ".png"}]
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as f:
        json.dump(job, f)
    subprocess.run(["ssh", "spark", "mkdir -p /tmp/me-concept"], check=True)
    subprocess.run(["scp", "-q", FLUX, "spark:/tmp/me-concept/flux.py"], check=True)
    subprocess.run(["scp", "-q", f.name, "spark:/tmp/me-concept/job.json"], check=True)
    subprocess.run(["ssh", "spark", "cd /tmp/me-concept && python3 flux.py job.json"], check=True)
    subprocess.run(["scp", "-q", f"spark:{remote}.png", out], check=True)
    return out


def concept_via_model(task, cfg, out):
    api = provider(cfg.get("provider", "yalr"))
    resp = chat(api, {"model": cfg["model"], "max_tokens": 600, "messages": [{"role": "user", "content":
        "Describe in 6 short lines the exact look of: " + task["prompt"]}]})
    return resp["choices"][0]["message"]["content"]
