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

`finish_grpo2.sh` waits for the 100 step run, evaluates it on the first 100 test pairs under `/tmp/gpu27.lock`, adds the row to the results table above and fast-forward pushes (`--test` runs it on the SFT adapter without pushing). It runs as the user unit `gcad-563-grpo2`.

### 1000 steps on the DGX Spark: does not fit

The Spark has 121 GB unified memory, 102 GB used and about 15 GB free next to Frigate, Jellyfin, Home Assistant and the other containers, no torch or peft install, and an aarch64 CPU with no `gcad` or `gcad export` build for the reward. Training needs about 17 GB for the 4B bf16 model plus the frozen anchor activations, and the reward needs `gcad`. The 1000 step run (about 3 to 4 minutes a step on the shared desktop GPU, 50 to 65 hours) is not queued on the desktop GPU either. It needs a free GPU slot; resume from `lora_grpo2` with `--adapter` pointing at it.

## Concept-image inputs

`prepare_concept_sft.py` builds `out/vlm_concept/{train,val,test}.jsonl` from the #553 manifest (`manifest.synthetic.jsonl`, real and synthetic concepts) and `kept_img2img.jsonl`, keeping only objects that also have a verified pair, and mixing in clean four-view examples at 20 per concept example. Config: `tools/vlm/concept_sft.json`.

Result: 1 pair with a concept image (train 1, val 0, test 0), plus 20 clean examples per split as filler. The #553 concepts cover ABO, Kenney, Poly Haven, GSO and Thingi10k meshes and 5 zero-to-cad objects, but the pairs are all zero-to-cad CAD parts (25,325), and only 1 of those has a concept. Concept training needs concept images for the pairs themselves: render each train pair's GLB with `bench/concept-synth/render_all.py` and run `flux_generate.py` on a selection of a few thousand train pairs. That and the SFT run come after the GRPO v2 eval frees the GPU.

Generation for the pairs runs unattended: `bench/concept-synth/pick_pairs.py` selects 4,433 pairs (2,000 train stratified by op mix, all val and test), `tools/vlm/generate_pair_concepts.sh` (unit `gcad-563-concepts-gen`) renders them on the CPU and runs `flux_generate.py` on `spark` one job at a time under `/tmp/flux-spark.lock`, and `tools/vlm/finish_concepts.sh` (unit `gcad-563-concepts-finish`) then filters, runs `prepare_concept_sft.py`, replaces the Result line above with the counts, and pushes. Data is in `~/gcad-datasets/out/synthetic_pairs`. The concept SFT training is not started.

## Not done here

- Minds-eye judge v2 on the 8 tuning tasks and the comparison with qwen3.8-27b plus the skill. Those need the judge on yalr (spend) and the shared vLLM on port 8001. The test-split numbers above use the shape distance only.
- Better GRPO: several prompts per step, a KL penalty to the SFT policy, and a larger run.
- Concept-image inputs (real plus #553 synthetic). The SFT inputs are clean renders only.
