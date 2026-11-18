#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
BRIEF="${BRIEF_OUT:-$HOME/gcad-datasets/out/vlm_brief}"
EI="${EI_OUT:-$HOME/gcad-datasets/out/vlm_ei}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
START="${START:-$EI/r1/lora}"
NAME="${NAME:-dpo1}"
SAMPLES="${SAMPLES:-$EI/r1/samples.jsonl}"
BETA="${BETA:-0.1}"
ALPHA="${ALPHA:-0.5}"
LR="${LR:-2e-5}"
STEPS="${STEPS:-0}"
MIN_FREE_MB="${MIN_FREE_MB:-36000}"

if [ "${1:-}" = "--test" ]; then
  NAME=dpo_test
  STEPS=2
  export LIMIT=3
fi

R="$EI/$NAME"
mkdir -p "$R"
exec >>"$R/run.log" 2>&1
echo "== $(date -Is) $NAME from $START"

wait_for_memory() {
  while true; do
    free=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits | head -1)
    [ "$free" -ge "$MIN_FREE_MB" ] && return
    echo "$(date -Is) gpu free ${free} MB, need ${MIN_FREE_MB}"
    sleep 120
  done
}

export PATH="$HOME/.cargo/bin:$PATH"
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
export HF_HUB_OFFLINE=1
cd "$HERE" || exit 1

if [ ! -f "$R/train.done" ]; then
  rm -f "$R/lora_log.jsonl"
  flock "$LOCK" bash -c "$(declare -f wait_for_memory); MIN_FREE_MB=$MIN_FREE_MB; wait_for_memory; exec '$PY' dpo.py --adapter '$START' --data '$BRIEF/mixed_train.jsonl' --samples '$SAMPLES' --out '$R/lora' --beta $BETA --alpha $ALPHA --lr $LR --steps $STEPS" || exit 1
  touch "$R/train.done"
fi
"$HERE/eval_all.sh" "$R/lora/policy" "$R" || exit 1
echo "== $(date -Is) $NAME finished"
