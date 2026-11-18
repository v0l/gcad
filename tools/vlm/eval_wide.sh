#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
BRIEF="${BRIEF_OUT:-$HOME/gcad-datasets/out/vlm_brief}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
OUT="$1"
OFFSET="${OFFSET:-100}"
LIMIT="${LIMIT:-200}"
shift
export PATH="$HOME/.cargo/bin:$PATH"
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
export HF_HUB_OFFLINE=1
cd "$HERE" || exit 1
mkdir -p "$OUT"
for spec in "$@"; do
  name="${spec%%=*}"
  adapter="${spec#*=}"
  [ -s "$OUT/${name}_images.jsonl" ] || flock "$LOCK" "$PY" eval.py --adapter "$adapter" --offset "$OFFSET" --limit "$LIMIT" --out "$OUT/${name}_images.jsonl" | tail -1
  [ -s "$OUT/${name}_briefs.jsonl" ] || flock "$LOCK" "$PY" eval_brief.py --adapter "$adapter" --briefs "$BRIEF/briefs.jsonl" --offset "$OFFSET" --limit "$LIMIT" --out "$OUT/${name}_briefs.jsonl" | tail -1
done
echo "== $(date -Is) wide eval finished"
