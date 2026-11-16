# VLM fine-tune on gcad pairs

Image to `.gcad` with a LoRA fine-tune of **Qwen3.5-4B-Base** on the verified pairs from `docs/training-data.md` ("gcad pairs"). This is the only model we train. Code is in `tools/vlm/`; `prompt_format.BASE` is the default base for `sft.py`, `eval.py` and `grpo.py`. Data and adapters stay in `~/gcad-datasets/out/vlm/`, never in git. The current adapter is `out/vlm/lora_qwen35`.

The first rounds (#562, #563) used Qwen3-VL-4B-Instruct. Those runs are kept below as history only; nothing new is trained on it.

## Base model

The pair statistics: 25,325 kept pairs (22,180 train, 1,246 val, 1,187 test after the commercial-OK and at most 60 lines filter), median 12 lines, p90 29. Ops are dominated by `extrude`, `plane`, `rect`, `chamfer`, `hole`, `fillet`.

`Qwen/Qwen3.5-4B-Base` (`Qwen3_5ForConditionalGeneration`, natively multimodal: `vision_config` with a 24 layer tower, no deepstack; hybrid linear and full attention language model) ships no chat template, so the plain format in `prompt_format.py` is used: four `<|vision_start|><|image_pad|><|vision_end|>` blocks, the prompt, a blank line, then the program and `<|endoftext|>` (the stop token at eval and in GRPO sampling). LoRA rank 32 on all language-model projections (42M trainable), bf16, 256 px views. It needs transformers 5 and `fla-core` (the reference linear attention is far too slow); they are in the venv `~/gcad-datasets/venv-vlm` (built with `--system-site-packages` over the system torch 2.12 cu130, then `pip install "transformers>=5" peft fla-core accelerate trimesh scipy`). Every script in `tools/vlm/` uses that venv by default (`VLM_PYTHON` overrides it).

## Pipeline

```
python tools/vlm/prepare_sft.py                                   # pairs.jsonl to train/val/test jsonl
tools/vlm/train_qwen35.sh                                         # LoRA SFT, 1400 steps, batch 2 x 8, lr 2e-4
python tools/vlm/eval.py --adapter out/vlm/lora_qwen35 --limit 100 --out results.jsonl
python tools/vlm/grpo.py --adapter out/vlm/lora_qwen35 --out DIR  # shape-distance reward, KL to the SFT
```

`train_qwen35.sh` waits for 24 GB of free GPU memory, then trains under `/tmp/gpu27.lock` with `PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True`; micro-batch 2 with 8 accumulation steps fits beside the vLLM on port 8001.

Input is four 256 px views (iso, front, top, right) and one fixed prompt; the target is the program text. `reward.py` builds a prediction with `gcad export` and compares it to the reference mesh with `tools/datasets/shape_distance.py`. Reward is 0 for a program that does not build, otherwise `0.1 + 0.9 * shape * volume`, where `shape` falls linearly to 0 at a chamfer of 5% of the bbox diagonal (after a 0.2% noise floor) and `volume` is `1 - volume_error`. Tests: `tools/vlm/test_*.py`.

## Results (first 100 test pairs, greedy, 700 new tokens)

| model | builds (`gcad export`) | match (volume within 1% and chamfer within 0.5%) | mean reward | median chamfer | median volume error |
|---|---|---|---|---|---|
| base Qwen3.5-4B-Base | 1% | 0% | 0.00 | 16.65% | 3206.9% |
| Qwen3.5-4B-Base SFT, 1400 steps | 90% | 22% | 0.51 | 1.46% | 11.1% |

Rows added by the finishers land under the SFT row above. The Qwen3-VL-4B-Instruct history:

| Qwen3-VL-4B-Instruct (history, not trained further) | builds | match | mean reward | median chamfer | median volume error |
|---|---|---|---|---|---|
| base | 0% | 0% | 0.00 | | |
| SFT, 1400 steps | 87% | 19% | 0.51 | 0.81% | 11.4% |
| SFT then GRPO, 120 steps (1 prompt per step, no KL) | 89% | 7% | 0.46 | 1.55% | 22.6% |
| GRPO v2, 8 prompts, KL 0.04, 100 steps | 91% | 22% | 0.53 | 0.97% | 13.8% |
| GRPO v2, step 60 checkpoint | 88% | 20% | 0.50 | 1.00% | 13.3% |
| no-dimension SFT adapter, dimensions in the test prompt | 75% | 22% | 0.54 | 0.41% | 2.5% |
| SFT with part dimensions in the prompt, 1400 steps | 84% | 27% | 0.65 | 0.36% | 1.7% |

The GRPO v2 and part-dimension recipes both helped on Qwen3-VL, so they are the next runs on Qwen3.5-4B-Base: `grpo.py` and `finish_dims.sh` default to it now.

SFT loss went from about 1.1 to 0.2. The failing builds are mostly `combine ... mode=cut` and `hole` placement errors.

## Part dimensions in the prompt (#565)

A person asking for a part gives sizes, not four renders alone. `tools/vlm/part_dims.py` reads a pair's `.gcad` and writes the dimensions as plain English in mm, rounded to two decimals at most:

- the base profile (rect w x h, circle diameter, n-gon across corners, polyline overall size, revolve maximum diameter and length, loft end sections) and its extrude depth;
- bosses and pockets (size and depth), slots, corner radius;
- holes: count, diameter, depth or through, counterbore and countersink, and the spacing when the points form a row, a pair or a regular grid;
- fillets and chamfers with their size and the edges in words (vertical corner edges, top edges, all edges);
- shell wall thickness.

Ops it cannot phrase (anything else, for example a `revolve` with no profile) are listed in `Spec.unphrased` and left out; the spec never guesses. The base profile, and the loft and revolve sizes, are always kept. Every other clause (hole diameter, depth, pattern, edge size, pocket depth and so on) is dropped with probability `p` (default 0.3) from a `random.Random` seeded by seed and pair id, so a given pair always gets the same spec. Lead phrases and clause wordings are picked from a few templates, and a spec is either one sentence or a bullet list.

```
The images show one part from four views: iso, front, top and right. Write the .gcad program, one operation per line, that builds it. Dimensions in mm: rectangular base 80 x 40, 30 mm thick; four holes, 6 mm diameter, through, on a 40 x 24 pattern; 1 mm chamfer on the vertical corner edges. Reply with the program only.
```

`prepare_sft.py --dims` and `eval.py --dims` (with `--dims-p` and `--dims-seed`) use it; without the flag the old prompt is used. Test pairs get their spec the same way with the fixed seed 1, so the same 100 test pairs see the same sizes in every run. A program that gives an empty spec keeps the old prompt. Over 5000 pairs from the kept set no spec was empty and no spec held a number that is not in (or twice, or a difference of, numbers in) its program. Tests: `tools/vlm/test_part_dims.py`.

`finish_dims.sh` prepares the data in `out/vlm_dims_<base>`, waits for 36 GB free on the GPU, trains under `/tmp/gpu27.lock` (rank 32, 1400 steps, batch 2 x 8, 256 px), evaluates on the first 100 test pairs, adds the rows above and pushes. Its default base is now Qwen3.5-4B-Base; the row above is from the earlier Qwen3-VL run.

## GRPO (Qwen3-VL history)

120 steps, one training image per step, group of 8 samples at temperature 0.9, lr 2e-5, group-normalised advantage, no KL term. It made the model worse on the test split: reward on the training prompts stayed flat (about 0.43, 0.45, 0.35 over three blocks of 40 steps) and test match fell from 19% to 7%. One prompt per step is too noisy and there is no reference-policy anchor.

## GRPO v2

`grpo.py` takes `--prompts` (default 8 training images per step, `--group` samples each), `--beta` (default 0.04), `--anchor` (the KL reference adapter, default the starting adapter) and `--steps` (default 1000). The KL term is the k3 estimator against a frozen copy of the SFT adapter loaded as a second PEFT adapter, so the anchor costs no extra base weights. The loss is normalised by the number of samples with non-zero advantage across all prompts. Output goes to `--out` with its log at `<out>_log.jsonl`. It uses the same prompt format as `sft.py` and `eval.py` (plain for Qwen3.5-4B-Base), stops sampling at `<|endoftext|>`, and cuts each completion at its first stop token before scoring and before the policy and anchor log-probs. Tests: `tools/vlm/test_grpo.py`, `tools/vlm/test_prompt_format.py`.

On Qwen3-VL, 100 steps at 8 prompts x 8 samples, `lr 1e-5`, `beta 0.04` gave 91% build and 22% match (from 87% and 19%), about 3 minutes a step on the shared RTX PRO 6000. That recipe is the next run on Qwen3.5-4B-Base, from `out/vlm/lora_qwen35`. `smoke_grpo_qwen35.sh` runs 2 steps of it under the GPU lock to prove the path before a long run.

### 1000 steps on the DGX Spark: does not fit

The Spark has 121 GB unified memory, 102 GB used and about 15 GB free next to Frigate, Jellyfin, Home Assistant and the other containers, no torch or peft install, and an aarch64 CPU with no `gcad` build for the reward. A 1000 step run (50 to 65 hours on the shared desktop GPU) needs a free GPU slot.

## Concept-image inputs

`prepare_concept_sft.py` builds `out/vlm_concept/{train,val,test}.jsonl` from the #553 manifest (`manifest.synthetic.jsonl`, real and synthetic concepts) and `kept_img2img.jsonl`, keeping only objects that also have a verified pair, and mixing in clean four-view examples at 20 per concept example. Config: `tools/vlm/concept_sft.json`.

Result: 1 pair with a concept image (train 1, val 0, test 0), plus 20 clean examples per split as filler. The #553 concepts cover ABO, Kenney, Poly Haven, GSO and Thingi10k meshes and 5 zero-to-cad objects, but the pairs are all zero-to-cad CAD parts (25,325), and only 1 of those has a concept. Concept training needs concept images for the pairs themselves: render each train pair's GLB with `bench/concept-synth/render_all.py` and run `flux_generate.py` on a selection of a few thousand train pairs. That and the concept SFT run (on Qwen3.5-4B-Base from `lora_qwen35`, `tools/vlm/concept_sft.json`) come after generation finishes.

Generation for the pairs runs unattended: `bench/concept-synth/pick_pairs.py` selects 4,433 pairs (2,000 train stratified by op mix, all val and test), `tools/vlm/generate_pair_concepts.sh` (unit `gcad-563-concepts-gen`) renders them on the CPU and runs `flux_generate.py` on `spark` one job at a time under `/tmp/flux-spark.lock`, and `tools/vlm/finish_concepts.sh` (unit `gcad-563-concepts-finish`) then filters, runs `prepare_concept_sft.py`, replaces the Result line above with the counts, and pushes. Data is in `~/gcad-datasets/out/synthetic_pairs`. The concept SFT training is not started.

## Minds-eye (evaluation only)

Minds-eye is a held-out evaluation. Its tasks are never training data, and nothing is generated from them for training: the model has to handle any user prompt. 8 tuning tasks (chest, sword, rocket, bracket, cottage, fox, knight, tractor), 2 seeds, judge `v2:code-x-high`, render on, plan off. The 27b is the local vLLM on port 8001 with the skill. Configs: `bench/minds-eye/configs/local-render.json`, `opus-render.json`.

| config | runs | built | judge v2 | rubric | hard | tokens/run | seconds | pairwise |
|---|---|---|---|---|---|---|---|---|
| local-render (qwen3.8-27b + skill) | 15 | 47% | 4.9 | 0.81 | 3.88 | 714k | 683 | reference |
| opus-render (claude-opus-5-5, thinking high) | 16 | 100% | 5.6 | 0.88 | 6.68 | 515k | 218 | 5-0 vs local |

Qwen3.5-4B-Base has not been run on minds-eye yet. The earlier Qwen3-VL SFT adapter built 1 of 16 runs (judge 1.0): it was trained on four renders in and a program out, never on a text brief or a tool loop, so it wrote `plane`/`rect`/`extrude` lines until the token limit. The opus row below is the bar.

## General gcad model (#627)

Training starts from the #564 adapter `out/vlm/lora_qwen35` on `prompt_format.BASE`. No training script reads `bench/minds-eye`, and none of its tasks, briefs, references, rubrics or runs enter training.

- `tools/vlm/briefs.py` captions the verified pairs into short requests with the local 27b (`BRIEF_URL`, `BRIEF_MODEL`, default vLLM on port 8001, thinking off), resumable through `out/vlm_brief/briefs.jsonl`, and writes `out/vlm_brief/{train,val,test}.jsonl` as text-in, program-out examples with five phrasings. Every brief goes through `tools/vlm/overlap.py`, which drops any brief that names a minds-eye task or ceiling model or has a word-set Jaccard of 0.6 or more with a minds-eye prompt, including briefs already in `briefs.jsonl`. Tests: `test_briefs.py`.
- `agent_api.run_api` writes the full transcript (tool calls, results, viewed images saved under `images/`) as the last line of `agent.jsonl`. This is for evaluation logs only.
- `tools/vlm/trajectories.py RUNS.jsonl --out traj.jsonl` converts the model's own self-correction runs on general briefs, each `{id, brief, built, reward, transcript}` with reward from `reward.py` against the known target program, into multi-turn examples. It keeps built runs with reward at least 0.5, and refuses input under `bench/minds-eye`, briefs that overlap minds-eye and transcripts with minds-eye images. Another model's runs may be used the same way, on general briefs only, with the user's approval. Tests: `test_trajectories.py`.
- `tools/vlm/train_agent.py` is the multi-turn LoRA trainer for that format (Qwen chat markup, tool calls as `<tool_call>` JSON in the assistant turn, tool results as `<tool_response>` user turns, viewed images as `<|image_pad|>` where they were viewed). Loss is on assistant turns only, including the end marker (`agent_format.assistant_labels`). It refuses data under `bench/minds-eye`. Tests: `test_agent_format.py`. It has not been run.

Commands that wait for the user's go (GPU window or spend):

```
VLM_PY=~/gcad-datasets/venv-vlm/bin/python
$VLM_PY tools/vlm/briefs.py
flock /tmp/gpu27.lock $VLM_PY tools/vlm/train_agent.py --data traj.jsonl --out ~/gcad-datasets/out/vlm/lora_agent
cd bench/minds-eye && ./run run configs/<our-model>.json --sets tuning heldout --seeds 2
$VLM_PY tools/vlm/grpo.py --adapter ~/gcad-datasets/out/vlm/lora_agent --out ~/gcad-datasets/out/vlm/lora_agent_grpo
```

Open: the self-correction run generator (general brief in, tool loop, `reward.py` against the target) is not written; it needs the merged SFT served on vLLM.
