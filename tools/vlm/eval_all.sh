#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
BRIEF="${BRIEF_OUT:-$HOME/gcad-datasets/out/vlm_brief}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
ADAPTER="$1"
R="$2"
LIMIT="${LIMIT:-100}"
K="${K:-8}"
MIN_FREE_MB="${MIN_FREE_MB:-30000}"

wait_for_memory() {
  while true; do
    free=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits | head -1)
    [ "$free" -ge "$MIN_FREE_MB" ] && return
    echo "$(date -Is) gpu free ${free} MB, need ${MIN_FREE_MB}"
    sleep 120
  done
}

gpu() {
  flock "$LOCK" bash -c "$(declare -f wait_for_memory); MIN_FREE_MB=$MIN_FREE_MB; wait_for_memory; $*"
}

export PATH="$HOME/.cargo/bin:$PATH"
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
export HF_HUB_OFFLINE=1
cd "$HERE" || exit 1
mkdir -p "$R"

gpu "'$PY' eval.py --adapter '$ADAPTER' --limit $LIMIT --out '$R/eval_images.jsonl'" | tail -1 || exit 1
gpu "'$PY' eval_brief.py --adapter '$ADAPTER' --briefs '$BRIEF/briefs.jsonl' --limit $LIMIT --out '$R/eval_briefs.jsonl'" | tail -1 || exit 1
gpu "'$PY' passk.py --adapter '$ADAPTER' --results '$R/eval_images.jsonl' --out '$R/passk_images.jsonl' --k $K" | tail -1 || exit 1
gpu "'$PY' passk.py --adapter '$ADAPTER' --results '$R/eval_briefs.jsonl' --briefs '$BRIEF/briefs.jsonl' --out '$R/passk_briefs.jsonl' --k $K" | tail -1 || exit 1
"$PY" resample_eval.py "$R/eval_images.jsonl" --samples "$R/passk_images.jsonl" --out "$R/loop_images.jsonl" | tail -1
"$PY" resample_eval.py "$R/eval_briefs.jsonl" --samples "$R/passk_briefs.jsonl" --out "$R/loop_briefs.jsonl" | tail -1
"$PY" failures.py "images=$R/eval_images.jsonl" "briefs=$R/eval_briefs.jsonl"
