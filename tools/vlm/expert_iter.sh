#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
BRIEF="${BRIEF_OUT:-$HOME/gcad-datasets/out/vlm_brief}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
ROUND="${ROUND:-1}"
START="${START:-$OUT/lora_brief}"
EI="${EI_OUT:-$HOME/gcad-datasets/out/vlm_ei}"
N_BRIEFS="${N_BRIEFS:-1100}"
N_IMAGES="${N_IMAGES:-500}"
K="${K:-8}"
REPLAY="${REPLAY:-2000}"
STEPS="${STEPS:-300}"
LR="${LR:-5e-5}"
LIMIT="${LIMIT:-100}"
MIN_FREE_MB="${MIN_FREE_MB:-30000}"

if [ "${1:-}" = "--test" ]; then
  EI="$EI/test"
  N_BRIEFS=2
  N_IMAGES=1
  REPLAY=3
  STEPS=2
  LIMIT=3
fi

R="$EI/r$ROUND"
LORA="$R/lora"
mkdir -p "$R/sft"
exec >>"$R/run.log" 2>&1
echo "== $(date -Is) round $ROUND from $START"

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

PREVIOUS=()
for ((i = 1; i < ROUND; i++)); do
  [ -f "$EI/r$i/samples.jsonl" ] && PREVIOUS+=("$EI/r$i/samples.jsonl")
done

if [ ! -f "$R/samples.done" ]; then
  gpu "'$PY' expert_iter.py sample --adapter '$START' --data '$BRIEF/mixed_train.jsonl' --out '$R/samples.jsonl' --briefs $N_BRIEFS --images $N_IMAGES --k $K --seed $ROUND --exclude ${PREVIOUS[*]:-}" || exit 1
  touch "$R/samples.done"
fi
"$PY" expert_iter.py summary --samples "$R/samples.jsonl" | tee "$R/samples_summary.jsonl"
"$PY" expert_iter.py build --data "$BRIEF/mixed_train.jsonl" --samples "$R/samples.jsonl" --out "$R/sft/train.jsonl" --replay "$REPLAY" --seed "$ROUND" || exit 1

if [ ! -f "$LORA/adapter_config.json" ] || [ ! -f "$R/sft.done" ]; then
  rm -f "$R/sft_log_lora.jsonl"
  gpu "exec '$PY' sft.py --data '$R/sft' --adapter '$START' --out '$LORA' --steps $STEPS --batch 2 --accum 8 --lr $LR" || exit 1
  touch "$R/sft.done"
fi

LIMIT="$LIMIT" K="$K" MIN_FREE_MB="$MIN_FREE_MB" LOCK="$LOCK" "$HERE/eval_all.sh" "$LORA" "$R" || exit 1
echo "== $(date -Is) round $ROUND finished"
