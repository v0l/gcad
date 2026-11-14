#!/usr/bin/env bash
set -u
REPO="$1"
DOC_REL="$2"
MESSAGE="$3"
PY="$4"
shift 4
HERE="$(cd "$(dirname "$0")" && pwd)"
ATTEMPTS="${PUBLISH_ATTEMPTS:-8}"

attempt=1
while [ "$attempt" -le "$ATTEMPTS" ]; do
  git -C "$REPO" fetch -q origin || exit 1
  tmp=$(mktemp -d)
  git -C "$REPO" worktree add -q --detach "$tmp" origin/master || exit 1
  args=("$@")
  i=0
  while [ "$i" -lt "${#args[@]}" ]; do
    "$PY" "$HERE/add_result_row.py" --results "${args[$i]}" --doc "$tmp/$DOC_REL" --label "${args[$((i + 1))]}" || { git -C "$REPO" worktree remove --force "$tmp"; exit 1; }
    i=$((i + 2))
  done
  git -C "$tmp" add "$DOC_REL"
  git -C "$tmp" -c commit.gpgsign=false commit -qm "$MESSAGE" || { git -C "$REPO" worktree remove --force "$tmp"; exit 1; }
  if git -C "$tmp" push -q origin HEAD:master; then
    git -C "$tmp" log --oneline -1
    git -C "$REPO" worktree remove --force "$tmp"
    exit 0
  fi
  git -C "$REPO" worktree remove --force "$tmp"
  echo "push rejected, attempt $attempt, retrying on fresh origin/master"
  attempt=$((attempt + 1))
  sleep 2
done
exit 1
