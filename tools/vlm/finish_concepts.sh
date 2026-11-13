#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
SYNTH="$REPO/bench/concept-synth"
S="${PAIRS_SYNTH:-$HOME/gcad-datasets/out/synthetic_pairs}"
PY="${VLM_PYTHON:-/tmp/vlm/bin/python}"
UNIT="${GEN_UNIT:-gcad-563-concepts-gen}"
DOC="${DOC:-$REPO/docs/vlm-finetune.md}"
LOG="${LOG:-$S/finish_concepts.log}"
POLL="${POLL:-300}"
PUSH=1

if [ "${1:-}" = "--test" ]; then
  PUSH=0
  DOC="$S/vlm-finetune.test.md"
  LOG="$S/finish_concepts_test.log"
  UNIT=none
  cp "$REPO/docs/vlm-finetune.md" "$DOC"
fi

exec >>"$LOG" 2>&1
echo "== $(date -Is) start"

while [ "$UNIT" != none ] && systemctl --user is-active --quiet "$UNIT"; do
  echo "$(date -Is) generated $(python3 "$SYNTH/progress.py" "$S/selection.jsonl" "$S/concept_img2img") of $(grep -c . "$S/selection.jsonl")"
  sleep "$POLL"
done

cd "$SYNTH" || exit 1
python3 filter_iou.py --meta "$S/generated_img2img.jsonl" --kept "$S/kept_img2img.jsonl" --dropped "$S/dropped_img2img.jsonl" | tail -20 || exit 1
cd "$HERE" || exit 1
"$PY" prepare_concept_sft.py --kept "$S/kept_img2img.jsonl" --out "$HOME/gcad-datasets/out/vlm_concept" || exit 1
"$PY" concept_counts.py --summary "$HOME/gcad-datasets/out/vlm_concept/summary.json" --generated "$S/generated_img2img.jsonl" --kept "$S/kept_img2img.jsonl" --dropped "$S/dropped_img2img.jsonl" --doc "$DOC" || exit 1

if [ "$PUSH" = 1 ]; then
  cd "$REPO" || exit 1
  git add docs/vlm-finetune.md
  git -c commit.gpgsign=false commit -qm "Concept pair counts" || exit 1
  git fetch -q origin
  git rebase origin/master || exit 1
  git push origin HEAD:master || exit 1
  git log origin/master -1 --oneline
else
  echo "test mode: no commit, no push, doc at $DOC"
fi
echo "== $(date -Is) finished"
