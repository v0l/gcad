#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
NEED_MB="${NEED_MB:-30000}"
STEPS="${STEPS:-100}"
START="$OUT/lora_qwen35"
NEW="$OUT/lora_qwen35_grpo"
LOG="${LOG:-$OUT/grpo_qwen35.log}"
LABEL="Qwen3.5-4B-Base SFT then GRPO v2, 8 prompts, KL 0.04, $STEPS steps"
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
export PATH="$HOME/.cargo/bin:$PATH"

exec >>"$LOG" 2>&1
echo "== $(date -Is) start"
while true; do
  free=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits | head -1)
  [ "$free" -ge "$NEED_MB" ] && break
  echo "$(date -Is) waiting for $NEED_MB MB free, have $free"
  sleep 60
done
cd "$HERE" || exit 1
flock "$LOCK" "$PY" grpo.py --adapter "$START" --out "$NEW" --steps "$STEPS" --prompts 8 --group 8 --lr 1e-5 --beta 0.04 || exit 1
echo "== $(date -Is) grpo done, evaluating"
ADAPTER="$NEW" TARGET_STEPS=0 LABEL="$LABEL" RESULTS="$OUT/eval_qwen35_grpo.jsonl" LOG="$OUT/finish_qwen35_grpo.log" "$HERE/finish_grpo2.sh"
echo "== $(date -Is) finished"
