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
| #564 SFT adapter, text briefs | 32% | 8% | 0.18 | 1.33% | 7.8% |
| Brief SFT (from the #564 adapter, 1000 steps), text briefs | 75% | 19% | 0.42 | 1.68% | 7.4% |
| Brief SFT (from the #564 adapter, 1000 steps), image prompts | 86% | 18% | 0.50 | 0.98% | 7.7% |
| Qwen3.5-4B-Base SFT then GRPO v2, 8 prompts, KL 0.04, 100 steps | 91% | 21% | 0.51 | 1.38% | 15.8% |

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

## Build failures and the check-and-repair loop (#703)

`failures.py NAME=results.jsonl ...` buckets every output that does not build by its `gcad export` error:

| bucket | SFT, test pairs | GRPO v2, test pairs | brief SFT, text briefs | brief SFT, image prompts |
|---|---|---|---|---|
| boolean failure (cut, union or combine) | 8 | 7 | 15 | 12 |
| fillet or chamfer failure | 1 | 2 | 7 | 0 |
| timeout (40 s) | 0 | 0 | 2 | 0 |
| bad edge selector | 0 | 0 | 1 | 0 |
| missing argument (`rect 8`) | 0 | 0 | 0 | 1 |
| gcad crash | 0 | 0 | 0 | 1 |
| other (boolean, long message) | 1 | 0 | 0 | 0 |
| failures | 10 of 100 | 9 of 100 | 25 of 100 | 14 of 100 |

There are no syntax errors, unknown ops or empty outputs, so constrained decoding would fix nothing. Truncation is 4 outputs in 300, all repetition loops (one line repeated to the 700 token limit), not a missing stop. Most boolean failures are geometry: a `hole` or `mode=cut` sketched on a plane that misses the solid (`cut_misses.py`: 1 to 4 per set, where dropping the line builds), counterbores over a raised boss, and `fillet`/`chamfer ... all` on revolved or thin parts. `validate_data.py` rebuilt all 22,807 training targets with today's `gcad`: none fail.

The failures are near misses for the model, not gaps in what it knows. Sampling 8 programs at temperature 0.8 for every failed prompt (`passk.py`) gave at least one that builds for 10 of 10 test pairs, 24 of 25 text briefs and 14 of 14 image prompts.

`repair.py` is the loop: build the greedy program; on failure resample up to K; if none builds, drop the line `gcad` names in its error until it builds; and as a last resort return a stub block. `resample_eval.py` scores the loop from the `passk.py` samples. `tries` counts programs built, the first included:

| model, set | single shot | with the loop | match | mean reward | mean tries | how the failures were fixed |
|---|---|---|---|---|---|---|
| SFT, test pairs | 90% | 100% | 23% | 0.56 | 1.31 | 10 resample |
| brief SFT, text briefs | 75% | 100% | 21% | 0.53 | 1.71 | 24 resample, 1 line drop |
| brief SFT, image prompts | 86% | 100% | 20% | 0.56 | 1.35 | 14 resample |

Dropping lines alone, without resampling, also reaches 100% (`repair_eval.py`: match 23%, 20% and 19%), but it keeps the broken program's shape. Tests: `tools/vlm/test_repair.py`.

`infer.py` runs the loop on one request: `infer.py --brief "..."` or `infer.py --images iso.png front.png top.png right.png`, with `--adapter` (default `out/vlm_ei/r2/lora`) and `--k` (default 8). It writes the program that builds to stdout, or to `--out`, and its stage and tries to stderr. Every `gcad` build in the reward and the loop runs under a 4 GB address-space cap (`GCAD_MEM_MB`): one failing counterbore grew to about 9 GB uncapped, and under the cap it fails in a second with the same error. None of 330 training targets needs more.

### Expert iteration

`expert_iter.sh` (round `ROUND`, from adapter `START`) samples 8 programs at temperature 0.8 for 1100 text briefs and 500 image prompts from the training split (`expert_iter.py sample`, resumable, scored with `reward.py`), then builds the SFT set (`expert_iter.py build`): for each prompt the best stopped, built sample with reward 0.75 or more, two distinct ones when some of its samples failed, and the ground truth where too few samples qualify; plus 2000 ground-truth rows from prompts that were not sampled. It fine-tunes 300 steps at lr 5e-5 from `START`, then evaluates single shot, resampling and the loop on both test sets and buckets the failures. Later rounds skip prompts sampled in earlier rounds. Data comes from `out/vlm_brief/mixed_train.jsonl` and the training split only; `expert_iter.py` refuses data under `bench/minds-eye`. Tests: `tools/vlm/test_expert_iter.py`.

Round 1 from `out/vlm/lora_brief`: on the training prompts 68% of brief samples and 77% of image samples built; 2% of brief prompts and 0.6% of image prompts had no sample that built. The SFT set was 1314 of the model's own programs, 909 ground-truth fills and 2000 replay rows.

`dpo.py` (with `dpo.sh`) is a build-preference pass on the same samples: for each prompt with a failing sample, the best built sample (or the ground truth) is chosen and up to two distinct failing samples are rejected, 2016 pairs; loss is DPO against the frozen starting adapter plus `alpha` times the chosen NLL per token. Tests: `tools/vlm/test_dpo.py`.

| model | images single shot | images match | briefs single shot | briefs match | images loop (tries) | briefs loop (tries) |
|---|---|---|---|---|---|---|
| brief SFT (`out/vlm/lora_brief`), start | 86% | 18% | 75% | 19% | 100% (1.35) | 100% (1.71) |
| expert iteration round 1 (`out/vlm_ei/r1/lora`) | 91% | 19% | 73% | 19% | 100% (1.22) | 100% (1.72) |
| r1 then DPO, beta 0.1, alpha 0.5, lr 2e-5, 252 steps | 84% | 11% | 76% | 14% | 100% (1.53) | 100% (1.57) |
| r1 then DPO, beta 0.2, alpha 1.0, lr 5e-6, 120 steps | 90% | 18% | 74% | 18% | 100% (1.20) | 100% (1.57) |

DPO does not help: the strong run raised the preference margin to about 12 nats and lost a third of the match rate, the gentle one changed nothing. On 100 briefs the single shot moves by a few points run to run (round 1 fixed 7 of the starting adapter's failures and broke 9 others, with 18 failing in both), so a change under about 5 points is noise.

Round 2 starts from round 1 on 1600 new training prompts: 72% of brief samples and 81% of image samples built (from 68% and 77% in round 1). `eval_wide.sh` scores every adapter on 200 more test prompts per set (offset 100). Pooled with the first 100, single shot over 300 prompts:

| model | images build | images match | images reward | briefs build | briefs match | briefs reward |
|---|---|---|---|---|---|---|
| brief SFT (`out/vlm/lora_brief`), start | 82.7% | 17.7% | 0.48 | 78.0% | 20.3% | 0.45 |
| expert iteration round 1 | 87.7% | 19.0% | 0.50 | 79.3% | 20.3% | 0.47 |
| expert iteration round 2 (`out/vlm_ei/r2/lora`) | 87.0% | 19.7% | 0.52 | 78.7% | 22.0% | 0.48 |

The loop on the 200 extra prompts (8 resamples, then line drops):

| model, set | single shot | with the loop | match | mean reward | mean tries | fixed by |
|---|---|---|---|---|---|---|
| round 1, images | 86% | 100% | 20% | 0.56 | 1.46 | 24 resample, 4 line drop |
| round 1, briefs | 82.5% | 100% | 22.5% | 0.58 | 1.43 | 33 resample, 2 line drop |
| round 2, images | 86% | 100% | 21% | 0.58 | 1.34 | 26 resample, 2 line drop |
| round 2, briefs | 81% | 100% | 25.5% | 0.60 | 1.57 | 34 resample, 4 line drop |

Round 2 is the adapter to use: same build rate as round 1, best match and reward. Expert iteration lifted the image single shot by about 5 points and match by 2 points, but briefs by only 1 point, and the second round gave no more build rate. The failures left are the same kernel buckets as before (booleans 16 of 28 on images, 28 of 38 on briefs; fillets 9 and 8). Nudging every number on the failing line by 0.01 mm fixes 1 to 3 in each set (`nudge.py`), so they are not coincident faces. Most are cuts that miss the solid or meet it in a way the kernel cannot resolve (#721). On the 100 test pairs the image-only SFT (`out/vlm/lora_qwen35`) is 90% single shot and 100% with the loop (1.31 tries).

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

Result: 4433 concept images generated for pairs, 4052 kept by the silhouette filter and 381 dropped. Concept pairs: train 1844, val 1153, test 1056 (4053 in total), mixed with 20560 clean four-view examples.

Generation for the pairs runs unattended: `bench/concept-synth/pick_pairs.py` selects 4,433 pairs (2,000 train stratified by op mix, all val and test), `tools/vlm/generate_pair_concepts.sh` (unit `gcad-563-concepts-gen`) renders them on the CPU and runs `flux_generate.py` on `spark` one job at a time under `/tmp/flux-spark.lock`, and `tools/vlm/finish_concepts.sh` (unit `gcad-563-concepts-finish`) then filters, runs `prepare_concept_sft.py`, replaces the Result line above with the counts, and pushes. Data is in `~/gcad-datasets/out/synthetic_pairs`. The concept SFT training is not started.

## Minds-eye (evaluation only)

Minds-eye is a held-out evaluation. Its tasks are never training data, and nothing is generated from them for training: the model has to handle any user prompt. 8 tuning tasks (chest, sword, rocket, bracket, cottage, fox, knight, tractor), 2 seeds, judge `v2:code-x-high`, render on, plan off. The 27b is the local vLLM on port 8001 with the skill. Configs: `bench/minds-eye/configs/local-render.json`, `opus-render.json`.

| config | runs | built | judge v2 | rubric | hard | tokens/run | seconds | pairwise |
|---|---|---|---|---|---|---|---|---|
| local-render (qwen3.8-27b + skill) | 15 | 47% | 4.9 | 0.81 | 3.88 | 714k | 683 | reference |
| opus-render (claude-opus-5-5, thinking high) | 16 | 100% | 5.6 | 0.88 | 6.68 | 515k | 218 | 5-0 vs local |
| gcadvlm-r2 (Qwen3.5-4B, expert iteration round 2, repair loop) | 16 | 100% | 1.6 | 0.11 | 3.98 | 1k | 21 | 0-5 vs local |

The gcadvlm-r2 row is `out/vlm_ei/r2/lora` run through `tools/vlm/infer.py` on the task prompt (config `configs/gcadvlm-r2.json`, agent `vlm`: one greedy program, then the repair loop, no renders). Every run builds, but only the bracket scores (5 and 6, against opus 8 and 8): the judge calls it a clean flat L plate with countersunk holes, not a bent angle bracket. The other seven tasks score 1. The sword is a flat L bar, the chest, cottage, rocket and tractor are box-and-plate mechanical parts, and the knight fell through to the stub cube. The model writes small machined parts whatever it is asked for, because every training target is a CAD part from the pair datasets. Greedy decoding gives the same program for both seeds. The earlier Qwen3-VL SFT adapter built 1 of 16 runs (judge 1.0): it was trained on four renders in and a program out, never on a text brief or a tool loop, so it wrote `plane`/`rect`/`extrude` lines until the token limit.

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
