#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
S="${SYNTH_DIR:-$HOME/gcad-datasets/out/synthetic}"
MANIFEST="${MANIFEST:-$HOME/gcad-datasets/out/manifest.jsonl}"
PY="${EMB_PYTHON:-/tmp/emb/bin/python}"
SELECTION="${SELECTION:-$S/selection500.jsonl}"
META="${META:-$S/generated_img2img.jsonl}"
KEPT="${KEPT:-$S/kept_img2img.jsonl}"
DROPPED="${DROPPED:-$S/dropped_img2img.jsonl}"
README="${README:-$HERE/README.md}"
LOG="${LOG:-$S/finish_500.log}"
POLL="${POLL:-300}"
PUSH=1
TITLE="500 objects"

if [ "${1:-}" = "--test" ]; then
  PUSH=0
  TITLE="test run"
  SELECTION="$S/selection43.jsonl"
  KEPT="$S/kept_test.jsonl"
  DROPPED="$S/dropped_test.jsonl"
  LOG="$S/finish_500_test.log"
  README="$S/README.test.md"
  cp "$HERE/README.md" "$README"
fi

exec >>"$LOG" 2>&1
echo "== $(date -Is) start ($TITLE)"

total=$(grep -c . "$SELECTION")
progress() {
  "$PY" "$HERE/progress.py" "$SELECTION" "$S/concept_img2img"
}

while true; do
  done_n=$(progress)
  echo "$(date -Is) generated $done_n of $total"
  if [ "$done_n" -ge "$total" ]; then
    break
  fi
  if ! pgrep -f "flux_generate.py" >/dev/null; then
    echo "generator not running, finishing with $done_n of $total"
    break
  fi
  sleep "$POLL"
done

cd "$HERE" || exit 1
python3 filter_iou.py --meta "$META" --kept "$KEPT" --dropped "$DROPPED" | tail -40 || exit 1
python3 merge_manifest.py --manifest "$MANIFEST" --kept "$KEPT" --out "$S/../manifest.synthetic.jsonl" || exit 1

OUTLINE_JSON="$S/ablation_dino_500.json"
HF_HUB_OFFLINE=1 OMP_NUM_THREADS=4 "$PY" ablation.py --features dino --manifest "$S/../manifest.synthetic.jsonl" --kept "$KEPT" --out "$OUTLINE_JSON" || exit 1

python3 update_readme.py --readme "$README" --title "$TITLE" --total "$total" --kept "$KEPT" --dropped "$DROPPED" --ablation "$OUTLINE_JSON" || exit 1

if [ "$PUSH" = 1 ]; then
  cd "$REPO" || exit 1
  git add bench/concept-synth/README.md
  git -c commit.gpgsign=false commit -qm "Concept results for 500 objects" || exit 1
  git fetch -q origin
  git rebase origin/master || exit 1
  git push origin HEAD:master || exit 1
  git log origin/master -1 --oneline
else
  echo "test mode: no commit, no push, readme at $README"
fi
echo "== $(date -Is) finished"
