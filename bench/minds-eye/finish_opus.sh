#!/usr/bin/env bash
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(git -C "$HERE" rev-parse --show-toplevel)"
RESULTS="$HERE/results"
CONFIG="${CONFIG:-opus-render}"
LABEL="${LABEL:-opus-render (claude-opus-5-5, thinking high)}"
UNIT="${RUN_UNIT:-gcad-563-opus}"
AGAINST="${AGAINST:-$HOME/git/gcad/bench/minds-eye/results/2026-11-11-local-render.json}"
DOC_REL=docs/vlm-finetune.md
LOG="${LOG:-$HOME/gcad-datasets/out/vlm/finish_opus.log}"
POLL="${POLL:-60}"
PUSH=1

if [ "${1:-}" = "--test" ]; then
  PUSH=0
  UNIT=none
  LOG="$HOME/gcad-datasets/out/vlm/finish_opus_test.log"
fi

exec >>"$LOG" 2>&1
echo "== $(date -Is) start"
while [ "$UNIT" != none ] && systemctl --user is-active --quiet "$UNIT"; do
  echo "$(date -Is) waiting for $UNIT, $(grep -c '^  builds=' "$HOME/gcad-datasets/out/vlm/opus_minds_eye.log" 2>/dev/null || echo 0) of 16 runs"
  sleep "$POLL"
done

result="${RESULT:-$(ls -t "$RESULTS"/*-"$CONFIG".json 2>/dev/null | head -1)}"
if [ -z "$result" ] || [ ! -f "$result" ]; then
  echo "no result for $CONFIG"
  exit 1
fi
echo "result $result"

pair=""
if [ "$PUSH" = 1 ] && [ -f "$AGAINST" ]; then
  against_runs="$(dirname "$AGAINST")/runs/$(basename "$AGAINST" .json)"
  [ -e "$RESULTS/runs/$(basename "$against_runs")" ] || ln -s "$against_runs" "$RESULTS/runs/$(basename "$against_runs")"
  cd "$HERE" || exit 1
  JUDGE_VERSION=v2 ./run pairwise "$result" "$AGAINST" | tail -8 || exit 1
  pair=$(ls -t "$RESULTS"/*-pair-"$CONFIG"-vs-*.json | head -1)
fi

if [ "$PUSH" = 0 ]; then
  doc="$HOME/gcad-datasets/out/vlm/vlm-finetune.opus-test.md"
  cp "$REPO/$DOC_REL" "$doc"
  python3 "$HERE/minds_eye_row.py" --result "$result" --doc "$doc" --label "$LABEL" || exit 1
  echo "test mode: no pairwise, no commit, no push, doc at $doc"
  exit 0
fi

for attempt in 1 2 3 4 5 6 7 8; do
  git -C "$REPO" fetch -q origin || exit 1
  tmp=$(mktemp -d)
  git -C "$REPO" worktree add -q --detach "$tmp" origin/master || exit 1
  python3 "$HERE/minds_eye_row.py" --result "$result" --doc "$tmp/$DOC_REL" --label "$LABEL" --pairwise "$pair" || { git -C "$REPO" worktree remove --force "$tmp"; exit 1; }
  git -C "$tmp" add "$DOC_REL"
  git -C "$tmp" -c commit.gpgsign=false commit -qm "Opus minds-eye result" || { git -C "$REPO" worktree remove --force "$tmp"; exit 1; }
  if git -C "$tmp" push -q origin HEAD:master; then
    git -C "$tmp" log --oneline -1
    git -C "$REPO" worktree remove --force "$tmp"
    echo "== $(date -Is) finished"
    exit 0
  fi
  git -C "$REPO" worktree remove --force "$tmp"
  echo "push rejected, attempt $attempt"
  sleep 2
done
exit 1
