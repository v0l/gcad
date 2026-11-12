import argparse
import json
import os
import subprocess
import tempfile

DATA = os.path.expanduser("~/gcad-datasets")
BLENDER = os.path.join(DATA, "tools/blender-4.2.3-linux-x64/blender")
HERE = os.path.dirname(os.path.abspath(__file__))


def render(selection, outdir, batch=5):
    os.makedirs(outdir, exist_ok=True)
    records = [json.loads(l) for l in open(selection)]
    done = []
    for i in range(0, len(records), batch):
        jobs = []
        for m in records[i:i + batch]:
            name = m["id"].replace(":", "__").replace("/", "_").replace(" ", "_")
            out = os.path.join(outdir, name + ".png")
            if not os.path.exists(out):
                jobs.append({"id": m["id"], "glb": os.path.join(DATA, m["paths"]["glb"]), "out": out})
        if not jobs:
            continue
        with tempfile.TemporaryDirectory() as td:
            jf, rf = os.path.join(td, "jobs.json"), os.path.join(td, "res.json")
            json.dump(jobs, open(jf, "w"))
            subprocess.run(["systemd-run", "--user", "--wait", "--collect", "--quiet", "-p", "MemoryMax=6G", "-p", "MemorySwapMax=0", BLENDER, "-b", "--python", os.path.join(HERE, "render_clay.py"), "--", jf, rf],
                           check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=1800)
            if os.path.exists(rf):
                done.extend(json.load(open(rf)))
    return done


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("selection")
    ap.add_argument("--out", default=os.path.join(DATA, "out/synthetic/render"))
    ap.add_argument("--log", default=None)
    args = ap.parse_args()
    res = render(args.selection, args.out)
    bad = [r for r in res if "error" in r]
    print(len(res) - len(bad), "rendered", len(bad), "failed")
    if args.log:
        with open(args.log, "w") as f:
            for r in res:
                f.write(json.dumps(r) + "\n")


if __name__ == "__main__":
    main()
