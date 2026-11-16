import argparse
import json
import random
from pathlib import Path


def mix(brief_rows, image_rows, image_share, seed):
    n_images = int(len(brief_rows) * image_share / (1 - image_share))
    rng = random.Random(seed)
    images = rng.sample(image_rows, min(n_images, len(image_rows)))
    rows = brief_rows + images
    rng.shuffle(rows)
    return rows


def read(path):
    return [json.loads(l) for l in Path(path).read_text().splitlines() if l.strip()]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--briefs", required=True)
    parser.add_argument("--images", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--image-share", type=float, default=0.2)
    parser.add_argument("--seed", type=int, default=1)
    args = parser.parse_args()
    rows = mix(read(args.briefs), read(args.images), args.image_share, args.seed)
    with open(args.out, "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    print(len(rows), "rows")


if __name__ == "__main__":
    main()
