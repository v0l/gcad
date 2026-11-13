#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-/tmp/vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
STEPS=40
RESUME="$OUT/lora_grpo2/policy"
NEW="$OUT/lora_grpo2_resume"
LOG="${LOG:-$OUT/resume_grpo2.log}"
LABEL="GRPO v2, 100 steps"
FINISH_ARGS=""

if [ "${1:-}" = "--test" ]; then
  STEPS=1
  NEW="$OUT/lora_grpo2_resume_test"
  LOG="$OUT/resume_grpo2_test.log"
  LOCK=/tmp/gpu27-test.lock
  LABEL="test run (GRPO resume)"
  FINISH_ARGS="--test"
fi

exec >>"$LOG" 2>&1
echo "== $(date -Is) start, waiting for $LOCK"
export PATH="$HOME/.cargo/bin:$PATH"
cd "$HERE" || exit 1
flock "$LOCK" "$PY" grpo.py --adapter "$RESUME" --anchor "$OUT/lora" --out "$NEW" --steps "$STEPS" --prompts 8 --group 8 --lr 1e-5 --beta 0.04 --seed 61 || exit 1
echo "== $(date -Is) grpo done, evaluating"
ADAPTER="$NEW" TARGET_STEPS=0 LABEL="$LABEL" RESULTS="$OUT/eval_grpo2_resume.jsonl" LOG="$OUT/finish_grpo2_resume.log" "$HERE/finish_grpo2.sh" $FINISH_ARGS
echo "== $(date -Is) finished"
