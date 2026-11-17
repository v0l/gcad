import argparse
import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from repair import repair


def batch_resampler(draw):
    cache = []

    def next_sample():
        if not cache:
            cache.extend(draw())
        return cache.pop(0) if cache else ""

    return next_sample


def solve(generate, k):
    first = generate(1, 0.0)[0]
    return repair(first, batch_resampler(lambda: generate(k - 1, 0.8)), k)


def load(adapter):
    import torch
    from peft import PeftModel
    from transformers import AutoModelForImageTextToText, AutoProcessor

    from prompt_format import BASE

    processor = AutoProcessor.from_pretrained(BASE)
    model = AutoModelForImageTextToText.from_pretrained(BASE, dtype=torch.bfloat16, device_map={"": 0})
    if adapter:
        model = PeftModel.from_pretrained(model, adapter)
    model.eval()
    return model, processor


def generator(model, processor, text, image_paths, max_new_tokens=700):
    import torch
    from PIL import Image

    from eval import IMAGE_SIDE
    from prompt_format import BASE, completion_tokens, format_for, prompt_text, stop_ids

    fmt = format_for(BASE)
    stop = stop_ids(processor.tokenizer, fmt)[0]
    pad = processor.tokenizer.pad_token_id
    images = [Image.open(p).convert("RGB").resize((IMAGE_SIDE, IMAGE_SIDE)) for p in image_paths]
    batch = processor(text=[prompt_text(processor, fmt, len(images), text)], images=images or None, return_tensors="pt").to(model.device)

    def generate(n, temperature):
        kwargs = {"max_new_tokens": max_new_tokens, "eos_token_id": [stop], "pad_token_id": pad if pad is not None else stop, "num_return_sequences": n}
        kwargs.update({"do_sample": True, "temperature": temperature} if temperature > 0 else {"do_sample": False})
        with torch.no_grad():
            out = model.generate(**batch, **kwargs)
        comps = [completion_tokens(c.tolist(), stop, pad) for c in out[:, batch["input_ids"].shape[1]:]]
        return processor.tokenizer.batch_decode(comps, skip_special_tokens=True)

    return generate


def main():
    from pairs_data import PROMPT, VIEWS

    parser = argparse.ArgumentParser()
    parser.add_argument("--adapter", default=str(Path.home() / "gcad-datasets" / "out" / "vlm" / "lora_brief"))
    parser.add_argument("--brief", default="")
    parser.add_argument("--images", nargs="*", default=[], help=" ".join(VIEWS))
    parser.add_argument("--k", type=int, default=8)
    parser.add_argument("--out", default="")
    args = parser.parse_args()
    if bool(args.brief) == bool(args.images):
        parser.error("give --brief or --images, not both")
    if args.images and len(args.images) != len(VIEWS):
        parser.error(f"--images takes {len(VIEWS)} views: {' '.join(VIEWS)}")
    model, processor = load(args.adapter)
    result = solve(generator(model, processor, args.brief or PROMPT, args.images), args.k)
    if args.out:
        Path(args.out).write_text(result["program"])
    else:
        sys.stdout.write(result["program"])
    print(json.dumps({k: v for k, v in result.items() if k != "program"}), file=sys.stderr)
    sys.exit(0 if result["built"] else 1)


if __name__ == "__main__":
    main()
