#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
NEED_MB="${NEED_MB:-30000}"
LOG="${LOG:-$OUT/grpo_qwen35_smoke.log}"
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
flock "$LOCK" "$PY" grpo.py --adapter "$OUT/lora_qwen35" --out "$OUT/lora_qwen35_grpo_smoke" --steps 2 --prompts 2 --group 4 || exit 1
tail -2 "$OUT/lora_qwen35_grpo_smoke_log.jsonl"
echo "== $(date -Is) finished"
