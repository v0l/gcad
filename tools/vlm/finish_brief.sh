#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
BRIEF="${BRIEF_OUT:-$HOME/gcad-datasets/out/vlm_brief}"
PY="${VLM_PYTHON:-$HOME/gcad-datasets/venv-vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
STEPS="${STEPS:-1000}"
LIMIT=100
MIN_FREE_MB="${MIN_FREE_MB:-36000}"
PUSH=1

if [ "${1:-}" = "--test" ]; then
  PUSH=0
  STEPS=2
  LIMIT=3
  MIN_FREE_MB=0
  LOCK=/tmp/gpu27-test.lock
  BRIEF="$OUT/brief_test"
  LORA="$OUT/lora_brief_test"
else
  LORA="$OUT/lora_brief"
fi

mkdir -p "$BRIEF"
exec >>"$BRIEF/finish_brief.log" 2>&1
echo "== $(date -Is) start"

wait_for_memory() {
  while true; do
    free=$(nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits | head -1)
    if [ "$free" -ge "$MIN_FREE_MB" ]; then
      return
    fi
    echo "$(date -Is) gpu free ${free} MB, need ${MIN_FREE_MB}"
    sleep 120
  done
}

export PATH="$HOME/.cargo/bin:$PATH"
cd "$HERE" || exit 1

if [ "$PUSH" = 1 ]; then
  while systemctl --user is-active --quiet gcad-627-briefs.service; do
    echo "$(date -Is) waiting for captioning"
    sleep 300
  done
  "$PY" briefs.py --out "$BRIEF" --workers 6 || exit 1
else
  "$PY" briefs.py --out "$BRIEF" --limit 6 --workers 3 || exit 1
fi

"$PY" mix_data.py --briefs "$BRIEF/train.jsonl" --images "$OUT/train.jsonl" --out "$BRIEF/mixed_train.jsonl" || exit 1
mkdir -p "$BRIEF/sft"
cp "$BRIEF/mixed_train.jsonl" "$BRIEF/sft/train.jsonl"

if [ ! -f "$LORA/adapter_config.json" ]; then
  flock "$LOCK" bash -c "$(declare -f wait_for_memory); MIN_FREE_MB=$MIN_FREE_MB; wait_for_memory; PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True exec '$PY' sft.py --data '$BRIEF/sft' --adapter '$OUT/lora_qwen35' --out '$LORA' --steps $STEPS --batch 2 --accum 8 --lr 1e-4" || exit 1
fi

flock "$LOCK" "$PY" eval.py --adapter "$LORA" --limit "$LIMIT" --out "$BRIEF/eval_images.jsonl" | tail -3 || exit 1
flock "$LOCK" "$PY" eval_brief.py --adapter "$LORA" --briefs "$BRIEF/briefs.jsonl" --limit "$LIMIT" --out "$BRIEF/eval_briefs.jsonl" | tail -3 || exit 1
flock "$LOCK" "$PY" eval_brief.py --briefs "$BRIEF/briefs.jsonl" --adapter "$OUT/lora_qwen35" --limit "$LIMIT" --out "$BRIEF/eval_briefs_before.jsonl" | tail -3 || exit 1

IMG="Brief SFT (from the #564 adapter, $STEPS steps), image prompts"
TXT="Brief SFT (from the #564 adapter, $STEPS steps), text briefs"
BEFORE="#564 SFT adapter, text briefs"
if [ "$PUSH" = 1 ]; then
  "$HERE/publish_rows.sh" "$REPO" docs/vlm-finetune.md "Brief SFT results" "$PY" \
    "$BRIEF/eval_images.jsonl" "$IMG" "$BRIEF/eval_briefs.jsonl" "$TXT" "$BRIEF/eval_briefs_before.jsonl" "$BEFORE" || exit 1
else
  echo "test mode: no commit, no push"
fi
echo "== $(date -Is) finished"
