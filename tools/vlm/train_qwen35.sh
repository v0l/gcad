#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-/tmp/vlm/bin/python}"
export PYTHONPATH="${QWEN35_PKGS:-/tmp/vlm35pkgs}"
export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True
NEED_MB="${NEED_MB:-24000}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
LOG="${LOG:-$OUT/sft_qwen35.log}"

exec >>"$LOG" 2>&1
echo "== $(date -Is) start"
while true; do
  used=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits | head -1)
  total=$(nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits | head -1)
  free=$((total - used))
  if [ "$free" -ge "$NEED_MB" ]; then
    break
  fi
  echo "$(date -Is) waiting for $NEED_MB MB free, have $free"
  sleep 60
done

cd "$HERE" || exit 1
rm -f "$OUT/sft_log_lora_qwen35.jsonl"
exec flock "$LOCK" "$PY" sft.py --base Qwen/Qwen3.5-4B-Base --out "$OUT/lora_qwen35" --steps 1400 --batch 2 --accum 8
