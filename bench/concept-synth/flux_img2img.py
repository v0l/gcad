import argparse
import hashlib
import json
import os
import subprocess
import time
import urllib.parse
import urllib.request
import uuid

from PIL import Image

DATA = os.path.expanduser("~/gcad-datasets")
GENERATOR = "flux1-dev img2img (ComfyUI, spark)"
BG = (236, 236, 232)
STYLE = ("professional concept art of {subject}, single object centred on a plain light grey background, "
         "three-quarter view, vivid painterly digital illustration, rich colours and varied materials, "
         "clean game asset concept sheet style, no text")


def prompt_for(m):
    cat = m.get("category") or "object"
    cat = cat.replace("collection:", "").replace("-", " ").strip()
    if cat in ("uncategorised", "models", "cad", ""):
        cat = "object"
    return STYLE.format(subject="a " + cat)


def flatten(src, dst, size=1024):
    im = Image.open(src).convert("RGBA")
    bg = Image.new("RGBA", im.size, BG + (255,))
    bg.alpha_composite(im)
    bg.convert("RGB").resize((size, size), Image.LANCZOS).save(dst)


def workflow(image_name, prompt, seed, denoise, steps):
    return {
        "1": {"class_type": "UNETLoader", "inputs": {"unet_name": "flux1-dev.safetensors", "weight_dtype": "fp8_e4m3fn"}},
        "2": {"class_type": "DualCLIPLoader", "inputs": {"clip_name1": "t5xxl_fp8_e4m3fn.safetensors", "clip_name2": "clip_l.safetensors", "type": "flux"}},
        "3": {"class_type": "VAELoader", "inputs": {"vae_name": "flux_ae.safetensors"}},
        "4": {"class_type": "CLIPTextEncode", "inputs": {"text": prompt, "clip": ["2", 0]}},
        "5": {"class_type": "FluxGuidance", "inputs": {"conditioning": ["4", 0], "guidance": 3.5}},
        "6": {"class_type": "LoadImage", "inputs": {"image": image_name}},
        "7": {"class_type": "VAEEncode", "inputs": {"pixels": ["6", 0], "vae": ["3", 0]}},
        "8": {"class_type": "KSampler", "inputs": {"model": ["1", 0], "seed": seed, "steps": steps, "cfg": 1.0, "sampler_name": "euler",
                                                    "scheduler": "simple", "positive": ["5", 0], "negative": ["5", 0],
                                                    "latent_image": ["7", 0], "denoise": denoise}},
        "9": {"class_type": "VAEDecode", "inputs": {"samples": ["8", 0], "vae": ["3", 0]}},
        "10": {"class_type": "SaveImage", "inputs": {"images": ["9", 0], "filename_prefix": "synth"}},
    }


class Comfy:
    def __init__(self, host):
        self.host = host
        self.base = "http://127.0.0.1:18188"
        self.tunnel = subprocess.Popen(["ssh", "-N", "-L", "18188:127.0.0.1:8188", host])
        for _ in range(20):
            try:
                urllib.request.urlopen(self.base + "/system_stats", timeout=2)
                break
            except Exception:
                time.sleep(0.5)

    def close(self):
        self.tunnel.terminate()

    def upload(self, path, name):
        boundary = uuid.uuid4().hex
        data = open(path, "rb").read()
        body = (f"--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; filename=\"{name}\"\r\n"
                "Content-Type: image/png\r\n\r\n").encode() + data + (
            f"\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"overwrite\"\r\n\r\ntrue\r\n--{boundary}--\r\n").encode()
        req = urllib.request.Request(self.base + "/upload/image", data=body,
                                     headers={"Content-Type": "multipart/form-data; boundary=" + boundary})
        return json.loads(urllib.request.urlopen(req).read())["name"]

    def run(self, graph):
        req = urllib.request.Request(self.base + "/prompt", data=json.dumps({"prompt": graph, "client_id": str(uuid.uuid4())}).encode(),
                                     headers={"Content-Type": "application/json"})
        pid = json.loads(urllib.request.urlopen(req).read())["prompt_id"]
        while True:
            hist = json.loads(urllib.request.urlopen(f"{self.base}/history/{pid}").read())
            if pid in hist:
                break
            time.sleep(1)
        img = hist[pid]["outputs"]["10"]["images"][0]
        q = urllib.parse.urlencode({"filename": img["filename"], "subfolder": img["subfolder"], "type": img["type"]})
        return urllib.request.urlopen(f"{self.base}/view?{q}").read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection")
    ap.add_argument("--render", default=os.path.join(DATA, "out/synthetic/render"))
    ap.add_argument("--out", default=os.path.join(DATA, "out/synthetic/concept"))
    ap.add_argument("--denoise", type=float, default=0.78)
    ap.add_argument("--steps", type=int, default=22)
    ap.add_argument("--host", default="spark")
    ap.add_argument("--meta", default=os.path.join(DATA, "out/synthetic/generated.jsonl"))
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    comfy = Comfy(args.host)
    try:
        with open(args.meta, "a") as meta:
            for line in open(args.selection):
                m = json.loads(line)
                name = m["id"].replace(":", "__").replace("/", "_").replace(" ", "_")
                src = os.path.join(args.render, name + ".png")
                dst = os.path.join(args.out, name + ".png")
                if not os.path.exists(src) or os.path.exists(dst):
                    continue
                flat = f"/tmp/{uuid.uuid4().hex}.png"
                flatten(src, flat)
                remote = comfy.upload(flat, "synth_in.png")
                os.remove(flat)
                prompt = prompt_for(m)
                seed = int(hashlib.sha1(m["id"].encode()).hexdigest()[:8], 16) % (2 ** 31)
                data = comfy.run(workflow(remote, prompt, seed, args.denoise, args.steps))
                open(dst, "wb").write(data)
                meta.write(json.dumps({"id": m["id"], "concept": dst, "render": src, "prompt": prompt, "generator": GENERATOR,
                                       "denoise": args.denoise, "steps": args.steps, "seed": seed}) + "\n")
                meta.flush()
                print("done", m["id"], flush=True)
    finally:
        comfy.close()


if __name__ == "__main__":
    main()
