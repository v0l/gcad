#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
SYNTH="$HERE/../../bench/concept-synth"
S="${PAIRS_SYNTH:-$HOME/gcad-datasets/out/synthetic_pairs}"
SELECTION="$S/selection.jsonl"
RENDER="$S/render"
CONCEPT="$S/concept_img2img"
META="$S/generated_img2img.jsonl"
LOG="${LOG:-$S/generate.log}"
mkdir -p "$S"
exec >>"$LOG" 2>&1
echo "== $(date -Is) start"
total=$(grep -c . "$SELECTION")

python3 "$SYNTH/render_all.py" "$SELECTION" --out "$RENDER" &
render_pid=$!

while true; do
  rendered=$(ls "$RENDER" 2>/dev/null | wc -l)
  generated=$(python3 "$SYNTH/progress.py" "$SELECTION" "$CONCEPT")
  echo "$(date -Is) rendered $rendered generated $generated of $total"
  if ! kill -0 "$render_pid" 2>/dev/null && [ "$generated" -ge "$rendered" ]; then
    break
  fi
  if [ "$rendered" -gt "$generated" ]; then
    flock /tmp/flux-spark.lock python3 "$SYNTH/flux_generate.py" "$SELECTION" --render "$RENDER" --out "$CONCEPT" --meta "$META" --mode img2img --host spark
    sleep 20
  else
    sleep 20
  fi
done
echo "== $(date -Is) generation finished"
