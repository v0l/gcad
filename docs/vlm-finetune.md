# VLM fine-tune on gcad pairs

Image to `.gcad` with a LoRA fine-tune of Qwen3-VL-4B-Instruct on the verified pairs from `docs/training-data.md` ("gcad pairs"). Code is in `tools/vlm/`. Data and adapters stay in `~/gcad-datasets/out/vlm/`, never in git.

## Base model

Qwen3-VL-4B-Instruct. The pair statistics: 25,325 kept pairs (22,180 train, 1,246 val, 1,187 test after the commercial-OK and at most 60 lines filter), median 12 lines, p90 29. Ops are dominated by `extrude`, `plane`, `rect`, `chamfer`, `hole`, `fillet`. Programs are short, so a 4B model with LoRA (rank 32 on all language-model projections, 66M trainable parameters) trains in bf16 on one GPU in about 17 GB. Qwen3.5-4B and its 3D-Coder finetune (#536) were not tried: the Qwen3-VL processor was available with a standard vision path and the 3D-Coder is Three.js-trained.

## Pipeline

```
python tools/vlm/prepare_sft.py                      # pairs.jsonl to train/val/test chat jsonl
python tools/vlm/sft.py --steps 1400 --accum 4       # LoRA SFT, batch 4 x 4, lr 2e-4, 256 px images
python tools/vlm/eval.py --adapter DIR --limit 100 --out results.jsonl
python tools/vlm/grpo.py --adapter DIR --out DIR2    # shape-distance reward
```

Input is four 256 px views (iso, front, top, right) and one fixed prompt; the target is the program text. `reward.py` builds a prediction with `gcad export` and compares it to the reference mesh with `tools/datasets/shape_distance.py`. Reward is 0 for a program that does not build, otherwise `0.1 + 0.9 * shape * volume`, where `shape` falls linearly to 0 at a chamfer of 5% of the bbox diagonal (after a 0.2% noise floor) and `volume` is `1 - volume_error`. Tests: `tools/vlm/test_*.py`.

## Results (first 100 test pairs, greedy, 700 new tokens)

| model | builds (`gcad export`) | match (volume within 1% and chamfer within 0.5%) | mean reward | median chamfer | median volume error |
|---|---|---|---|---|---|
| base Qwen3-VL-4B | 0% | 0% | 0.00 | | |
| SFT, 1400 steps (about 100 minutes on the RTX PRO 6000) | 87% | 19% | 0.51 | 0.81% | 11.4% |
| SFT then GRPO, 120 steps | 89% | 7% | 0.46 | 1.55% | 22.6% |

SFT loss went from about 1.1 to 0.2. The failing builds are mostly `combine ... mode=cut` and `hole` placement errors.

## GRPO

120 steps, one training image per step, group of 8 samples at temperature 0.9, lr 2e-5, group-normalised advantage, no KL term. It made the model worse on the test split: reward on the training prompts stayed flat (about 0.43, 0.45, 0.35 over three blocks of 40 steps) and test match fell from 19% to 7%. One prompt per step is too noisy and there is no reference-policy anchor. The script is in place; do not use the result adapter.

## GRPO v2

`grpo.py` now takes `--prompts` (default 8 training images per step, `--group` samples each), `--beta` (default 0.04) and `--steps` (default 1000). The KL term is the k3 estimator against a frozen copy of the SFT adapter loaded as a second PEFT adapter, so the anchor costs no extra base weights. The loss is normalised by the number of samples with non-zero advantage across all prompts. Output goes to `--out` with its log at `<out>_log.jsonl`. Tests: `tools/vlm/test_grpo.py`.

Measured cost on the shared RTX PRO 6000 (GPU shared with vLLM): about 3 minutes per step at 8 prompts x 8 samples, so 1000 steps is about 50 hours. A 100 step run (`lr 1e-5`, `beta 0.04`) was started to `~/gcad-datasets/out/vlm/lora_grpo2`; evaluate its checkpoint with `eval.py --adapter` before using it. The 1000 step run and the test-split result are not done.

## Not done here

- Minds-eye judge v2 on the 8 tuning tasks and the comparison with qwen3.8-27b plus the skill. Those need the judge on yalr (spend) and the shared vLLM on port 8001. The test-split numbers above use the shape distance only.
- Better GRPO: several prompts per step, a KL penalty to the SFT policy, and a larger run.
- Concept-image inputs (real plus #553 synthetic). The SFT inputs are clean renders only.
