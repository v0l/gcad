#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="$HOME/gcad-datasets/out/vlm"
PART1="$OUT/opus-render-part1.json"
TASKS=(bracket chest cottage fox knight rocket sword tractor)
REMAINING=(sword tractor rocket bracket cottage)
export JUDGE_VERSION=v2
cd "$HERE" || exit 1
./run run configs/opus-render.json --tasks "${REMAINING[@]}" --seeds 2 >> "$OUT/opus_minds_eye.log" 2>&1 || exit 1
cp "results/$(date +%F)-opus-render.json" "$OUT/opus-render-part2.json" || exit 1
python3 merge_results.py "$PART1" "$OUT/opus-render-part2.json" --tasks "${TASKS[@]}" --out results/opus-render-8tasks.json || exit 1
RESULT="$HERE/results/opus-render-8tasks.json" RUN_UNIT=none "$HERE/finish_opus.sh"
