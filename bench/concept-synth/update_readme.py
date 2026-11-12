import argparse
import json


def count(path):
    return sum(1 for line in open(path) if line.strip())


def section(title, total, kept, dropped, ablation):
    arms = ablation["summary_mae"]
    rows = "\n".join(
        f"| {a} | {arms[a]:.3f} |" for a in "ABC")
    return f"""## Results ({title})

Selection of {total} objects, img2img generator. {kept} kept and {dropped} dropped at IoU 0.75 (drops in `dropped_img2img.jsonl`). Arm B here uses {ablation['synthetic_objects']} synthetic objects.

DINOv2-small features, leave-one-object-out over {ablation['real_pairs']} real concept images, {ablation['steps']} steps, batch {ablation['batch']}, {ablation['seeds']} seeds. A is clean renders only, B is renders plus synthetic concepts, C is real concepts only.

| arm | MAE |
| --- | --- |
{rows}
"""


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--readme", required=True)
    ap.add_argument("--title", required=True)
    ap.add_argument("--total", type=int, required=True)
    ap.add_argument("--kept", required=True)
    ap.add_argument("--dropped", required=True)
    ap.add_argument("--ablation", required=True)
    args = ap.parse_args()
    text = open(args.readme).read()
    marker = f"## Results ({args.title})"
    if marker in text:
        text = text[:text.index(marker)].rstrip() + "\n"
    ablation = json.load(open(args.ablation))
    text = text.rstrip() + "\n\n" + section(args.title, args.total, count(args.kept), count(args.dropped), ablation)
    open(args.readme, "w").write(text)


if __name__ == "__main__":
    main()
