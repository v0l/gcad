#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
OUT="${VLM_OUT:-$HOME/gcad-datasets/out/vlm}"
PY="${VLM_PYTHON:-/tmp/vlm/bin/python}"
LOCK="${LOCK:-/tmp/gpu27.lock}"
DOC="${DOC:-$REPO/docs/vlm-finetune.md}"
STEPS="${STEPS:-1400}"
DEFAULT_BASE="Qwen/Qwen3-VL-4B-Instruct"
BASE="$DEFAULT_BASE"
LIMIT=100
MIN_FREE_MB="${MIN_FREE_MB:-36000}"
PUSH=1
TEST=0

while [ $# -gt 0 ]; do
  case "$1" in
    --test) TEST=1; PUSH=0 ;;
    --base) BASE="$2"; shift ;;
    *) echo "unknown argument $1"; exit 2 ;;
  esac
  shift
done

SLUG="$(basename "$BASE" | tr 'A-Z' 'a-z')"
DIMS_OUT="${DIMS_OUT:-$HOME/gcad-datasets/out/vlm_dims_$SLUG}"
NAME="$(basename "$BASE")"

if [ "$TEST" = 1 ]; then
  DIMS_OUT="$OUT/dims_test_$SLUG"
  LOCK=/tmp/gpu27-test.lock
  STEPS=2
  LIMIT=3
  MIN_FREE_MB=0
  DOC="$DIMS_OUT/vlm-finetune.test.md"
  mkdir -p "$DIMS_OUT"
  cp "$REPO/docs/vlm-finetune.md" "$DOC"
fi

mkdir -p "$DIMS_OUT"
exec >>"$DIMS_OUT/finish_dims.log" 2>&1
echo "== $(date -Is) start base $BASE"

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

"$PY" prepare_sft.py --dims --out "$DIMS_OUT" || exit 1

if [ ! -f "$DIMS_OUT/lora/adapter_config.json" ]; then
  flock "$LOCK" bash -c "$(declare -f wait_for_memory); MIN_FREE_MB=$MIN_FREE_MB; wait_for_memory; exec '$PY' sft.py --base '$BASE' --data '$DIMS_OUT' --out '$DIMS_OUT/lora' --steps $STEPS --accum 4" || exit 1
fi

flock "$LOCK" "$PY" eval.py --base "$BASE" --adapter "$DIMS_OUT/lora" --dims --limit "$LIMIT" --out "$DIMS_OUT/eval_dims.jsonl" | tail -3 || exit 1
"$PY" add_result_row.py --results "$DIMS_OUT/eval_dims.jsonl" --doc "$DOC" --label "SFT with part dimensions in the prompt, $NAME, $STEPS steps" || exit 1

if [ "$BASE" = "$DEFAULT_BASE" ]; then
  flock "$LOCK" "$PY" eval.py --adapter "$OUT/lora" --dims --limit "$LIMIT" --out "$DIMS_OUT/eval_nodims_adapter_dims_prompt.jsonl" | tail -3
  "$PY" add_result_row.py --results "$DIMS_OUT/eval_nodims_adapter_dims_prompt.jsonl" --doc "$DOC" --label "no-dimension SFT adapter, dimensions in the test prompt" || exit 1
fi

if [ "$PUSH" = 1 ]; then
  cd "$REPO" || exit 1
  git add docs/vlm-finetune.md
  git -c commit.gpgsign=false commit -qm "Dimension prompt SFT results ($NAME)" || exit 1
  git fetch -q origin
  git rebase origin/master || exit 1
  git push origin HEAD:master || exit 1
  git log origin/master -1 --oneline
else
  echo "test mode: no commit, no push, doc at $DOC"
fi
echo "== $(date -Is) finished"
