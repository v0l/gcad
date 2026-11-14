#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-/tmp/vlm/bin/python}"
export PYTHONPATH="${QWEN35_PKGS:-/tmp/vlm35pkgs}"
BASE="Qwen/Qwen3.5-4B-Base"
ADAPTER="${ADAPTER:-$OUT/lora_qwen35}"
TRAIN_UNIT="${TRAIN_UNIT:-gcad-564-sft}"
RESULTS="${RESULTS:-$OUT/eval_qwen35_sft.jsonl}"
BASE_RESULTS="${BASE_RESULTS:-$OUT/eval_qwen35_base.jsonl}"
DOC="${DOC:-$REPO/docs/vlm-finetune.md}"
LOG="${LOG:-$OUT/finish_qwen35.log}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
LIMIT=100
POLL="${POLL:-60}"
PUSH=1

if [ "${1:-}" = "--test" ]; then
  PUSH=0
  ADAPTER=/tmp/smoke564/lora
  RESULTS="$OUT/eval_qwen35_sft_test.jsonl"
  BASE_RESULTS="$OUT/eval_qwen35_base_test.jsonl"
  LOG="$OUT/finish_qwen35_test.log"
  LOCK=/tmp/gpu27-test.lock
  LIMIT=3
  TRAIN_UNIT=gcad-564-none
  DOC="$OUT/vlm-finetune.test.md"
  cp "$REPO/docs/vlm-finetune.md" "$DOC"
fi

exec >>"$LOG" 2>&1
echo "== $(date -Is) start"

while systemctl --user is-active --quiet "$TRAIN_UNIT"; do
  echo "$(date -Is) waiting for $TRAIN_UNIT, steps $(grep -c . "$OUT/sft_log_$(basename "$ADAPTER").jsonl" 2>/dev/null || echo 0)"
  sleep "$POLL"
done

if [ ! -f "$ADAPTER/adapter_model.safetensors" ]; then
  echo "no adapter at $ADAPTER"
  exit 1
fi

export PATH="$HOME/.cargo/bin:$PATH"
cd "$HERE" || exit 1
flock "$LOCK" "$PY" eval.py --base "$BASE" --adapter "$ADAPTER" --limit "$LIMIT" --out "$RESULTS" | tail -3 || exit 1
flock "$LOCK" "$PY" eval.py --base "$BASE" --limit "$LIMIT" --out "$BASE_RESULTS" | tail -3 || exit 1
if [ "$PUSH" = 1 ]; then
  "$HERE/publish_rows.sh" "$REPO" docs/vlm-finetune.md "Qwen3.5 base eval results" "$PY" "$RESULTS" "Qwen3.5-4B-Base SFT, 1400 steps" "$BASE_RESULTS" "base Qwen3.5-4B-Base" || exit 1
else
  "$PY" add_result_row.py --results "$RESULTS" --doc "$DOC" --label "Qwen3.5-4B-Base SFT, 1400 steps" || exit 1
  "$PY" add_result_row.py --results "$BASE_RESULTS" --doc "$DOC" --label "base Qwen3.5-4B-Base" || exit 1
  echo "test mode: no commit, no push, doc at $DOC"
fi
echo "== $(date -Is) finished"
