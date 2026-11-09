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
