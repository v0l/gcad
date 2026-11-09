# Minds-eye benchmark

Scores how good the 3D models are that agents write in gcad, so skill, tooling and model changes can be measured.

```
./run run configs/baseline.json --sets tuning --seeds 2 --limit 5
./run run configs/render.json --tasks chest sword --seeds 2
./run pairwise results/A.json results/B.json
./run report results/A.json results/B.json --out report.html
./run rating-sheet --n 20 --out ratings.json
./run agreement ratings.json
python3 -m unittest
```

Needs `gcad` on PATH, Python 3 with Pillow and numpy, and a provider in `~/.pi/agent/models.json` (default `yalr`).

## Tasks

`tasks/tuning.jsonl` (30) is for tuning the skill. `tasks/heldout.jsonl` (10) is never used to tune; run it only to confirm a final choice (`--sets heldout`). Each line has the prompt, a rubric, bounding box ratio rules (`num`/`den` axes, z up), whether symmetry is expected and a minimum part count.

## Configs

A config is a JSON file in `configs/`:

| key | meaning |
| --- | --- |
| `agent` | `api` (tool loop on a chat completions endpoint), `agentbench` (native agent session through the REST API), `cli` (a `command` list with `{prompt}`, for claude or codex) |
| `tier` / `model` | low, medium, high map to `code`, `code-high`, `code-x-high`; `model` overrides; `provider` picks another entry of models.json (a local model) |
| `skill` | path of a skill folder holding `SKILL.md` (default `skills/gcad`) |
| `render` | the agent may render and view PNGs, and is told to do so every few lines |
| `plan` | write a part list with sizes before detail |
| `reference` | a FLUX concept image from spark is given as `reference.png`, with a `compare` silhouette overlap tool |
| `budget` | `seconds`, `tokens`, `turns` |

Every run gets a fresh temp dir. The final `model.gcad`, agent log and renders are kept in `results/runs/<date>-<config>/<task>-s<seed>/`, and one `results/<date>-<config>.json` records tokens, wall time, tool calls, check result and scores.

## Scores

- Hard: builds clean, triangles, named parts, silhouette complexity (perimeter squared over area for the 6 view silhouettes), symmetry, bounding box ratios, use of mesh ops beyond blob, limb and block. `hard_score` is 0-10.
- Vision judge (`judge.py`, version `v1`, model from `JUDGE_MODEL`): per rubric item present or absent and an overall 1-10. Prompt and model are recorded in every result.
- Pairwise: blind A/B with random order on the same task and seed, summed into a win rate.
- Agreement: rate 20 runs yourself with `rating-sheet`, then `agreement` reports Pearson, Spearman, mean absolute error and the share within 1 point.

## Smoke run (2026-11-09)

5 tasks (chest, sword, rocket, bracket, cottage), 1 seed, medium tier, current SKILL.md, judge `v1:code-x-high`. Sample report: `docs/minds-eye-sample-report.html`. One seed per task means these numbers show the harness works, not a ranking.

| config | judge | rubric | hard | tokens | tool calls |
| --- | --- | --- | --- | --- | --- |
| baseline-medium | 5.6 | 0.63 | 5.4 | 575k | 26 |
| medium-render | 6.0 | 0.81 | 5.4 | 1065k | 45 |

Blind pairwise, render vs baseline: render won 5 of 5. Rubric coverage rose 0.63 to 0.81 for 1.9 times the tokens. On this set the agent chose solid ops for every task, including the chest, rocket and cottage, so `mesh_ops_used` was 0 and named parts stayed at 1. That is the "very basic models" symptom the issue describes.

## Reduced matrix (2026-11-09)

8 tuning tasks (chest, sword, rocket, bracket, cottage, fox, knight, tractor), 2 seeds (reference: 1 seed), judge `v1:code-x-high`. Old skill is the pre-#450 snapshot in `skills/gcad-pre450`, new skill is `skills/gcad` after #450. Report: `docs/minds-eye-sample-report.html`. Tokens are input plus output per run, mostly cached input.

| config | runs | judge | rubric | hard | tokens/run | seconds | pairwise vs medium-render |
| --- | --- | --- | --- | --- | --- | --- | --- |
| baseline (old skill, medium, no render) | 12 | 5.25 | 0.60 | 5.1 | 614k | 128 | 6-6 (50%) |
| medium-render (old skill) | 12 | 5.42 | 0.62 | 4.4 | 834k | 120 | reference |
| new-render (new skill, medium) | 12 | 5.42 | 0.61 | 5.1 | 369k | 133 | 4-7-1 (38%) |
| new-render-plan (new skill, medium) | 7 | 5.57 | 0.78 | 7.1 | 768k | 175 | 5-2 (71%) |
| new-render-low (new skill) | 2 | 4.0 | 0.50 | 8.8 | 972k | 300 | 0-2 (0%) |
| new-render-reference (new skill, FLUX concept) | 8 | 5.38 | 0.67 | 5.7 | 522k | 155 | 1-5 (17%) |

Not finished: new-render-plan stopped after 7 of 16 runs (fox, knight, chest, one sword), new-render-low after 2 (fox), and new-render-high was never run. The matrix passed the 15M token budget (29M including the reference runsso I stopped as instructed. The reference run covered fox, chest, tractor, sword, rocket, bracket, cottage and knight with 1 seed. Plan and low rows cover only the tasks listed, so their means are not comparable with the 12-run rows. Judge scores sit between 4 and 7 for everything, so differences below about 0.5 are noise at this sample size.

Findings:

- Render feedback alone does not move the judge score on this set (5.25 to 5.42), and its pairwise against no render is 6-6. The smoke run's 5-0 did not repeat. The earlier gain came from small samples.
- The #450 skill gives the same quality for 44% of the tokens of the old skill with render (369k against 834k), because the agent reads only the one reference it needs. It is slightly behind head to head (4-7-1), within noise.
- Plan first is the only change that moved rubric coverage (0.61 to 0.78) and hard score (5.1 to 7.1), and it beat render 5-2 pairwise. It covers only fox, knight, chest and one sword, so the claim needs the other tasks.
- Low tier is clearly worse on the one task it ran (4.0 against 6-7 for fox) and 2.3 times slower.
- The reference image did not help (pairwise 1-5). The agent used the compare tool on every run, but the silhouette overlap pulls it toward matching a FLUX picture, not toward a better model. FLUX on spark worked (about 50 s per concept).

Why the baseline makes solids: on every non-creature task, the agent in both skills used `rect`, `circle`, `extrude`, `body` and `color` only (`mesh_ops_used` is 0 for chest, sword, tractor and rocket). Only fox and knight runs used blobs and mesh ops (all new-skill fox and knight runs, about half of the old-skill ones). The new SKILL.md routes by a table whose first row is parts ("sketches, extrudes, holes") and whose mesh row says "characters, creatures", so props and vehicles go to solids. The runs read `parts.md` for tractor and sword and never opened `meshes.md`. Solid extrusions of cylinders and boxes give a legible but crude result, which is what the judge scores at 4 to 6.

Recommended default: new skill, render feedback on (it is cheap now, 369k tokens) and plan first, at medium. Run plan against the other 12 task types before making it the default, and add the changes below. Do not use low.

Judge agreement: `results/ratings.json` lists 20 runs to rate; fill in a 1-10 rating for each and run `./run agreement results/ratings.json`.

## Props and vehicles routing (#459)

SKILL.md now sends props, vehicles, buildings and game assets to `meshes.md` and tells the agent to block them out with `blob` and `limb`. `render-new`, medium, 1 seed.

| set | tasks | judge | rubric | hard | mesh_ops_used |
| --- | --- | --- | --- | --- | --- |
| tuning | chest, sword, tractor, rocket | 6.0 | 0.67 | 7.6 | 0.75 (was 0) |
| heldout | anvil, teapot, biplane, bulldozer, barn | 6.0 | 0.89 | 6.8 | 0.6 |

Judge scores are flat against the 5.4 baseline; the change moves mesh op use, not yet the score. One seed, so below 0.5 is noise.
