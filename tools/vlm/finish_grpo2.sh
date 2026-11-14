#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-/tmp/vlm/bin/python}"
ADAPTER="${ADAPTER:-$OUT/lora_grpo2}"
GRPO_LOG="${GRPO_LOG:-$OUT/lora_grpo2_log.jsonl}"
TARGET_STEPS="${TARGET_STEPS:-100}"
RESULTS="${RESULTS:-$OUT/eval_grpo2.jsonl}"
DOC="${DOC:-$REPO/docs/vlm-finetune.md}"
LOG="${LOG:-$OUT/finish_grpo2.log}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
LABEL="${LABEL:-GRPO v2, 8 prompts, KL 0.04, 100 steps}"
LIMIT=100
POLL="${POLL:-300}"
PUSH=1

if [ "${1:-}" = "--test" ]; then
  PUSH=0
  ADAPTER="$OUT/lora"
  RESULTS="$OUT/eval_grpo2_test.jsonl"
  LOG="$OUT/finish_grpo2_test.log"
  LOCK=/tmp/gpu27-test.lock
  LABEL="test run (SFT adapter)"
  LIMIT=3
  TARGET_STEPS=0
  DOC="$OUT/vlm-finetune.test.md"
  cp "$REPO/docs/vlm-finetune.md" "$DOC"
fi

exec >>"$LOG" 2>&1
echo "== $(date -Is) start ($LABEL)"

while true; do
  steps=$(grep -c . "$GRPO_LOG" 2>/dev/null || echo 0)
  echo "$(date -Is) grpo steps $steps of $TARGET_STEPS"
  if [ "$steps" -ge "$TARGET_STEPS" ]; then
    break
  fi
  if ! pgrep -f "grpo.py" >/dev/null; then
    echo "grpo.py not running, evaluating the last checkpoint at $steps steps"
    break
  fi
  sleep "$POLL"
done

export PATH="$HOME/.cargo/bin:$PATH"
cd "$HERE" || exit 1
ADAPTER=$("$PY" adapter_dir.py "$ADAPTER") || exit 1
echo "adapter $ADAPTER"
flock "$LOCK" "$PY" eval.py --adapter "$ADAPTER" --limit "$LIMIT" --out "$RESULTS" | tail -3 || exit 1
if [ "$PUSH" = 1 ]; then
  "$HERE/publish_rows.sh" "$REPO" docs/vlm-finetune.md "GRPO v2 eval result" "$PY" "$RESULTS" "$LABEL" || exit 1
else
  "$PY" add_result_row.py --results "$RESULTS" --doc "$DOC" --label "$LABEL" || exit 1
  echo "test mode: no commit, no push, doc at $DOC"
fi
echo "== $(date -Is) finished"
