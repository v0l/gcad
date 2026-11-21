#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
wxs=packaging/windows/gcad.wxs
fail=0
need() { grep -qF -- "$1" "$wxs" || { echo "missing in $wxs: $1"; fail=1; }; }
need 'Source="docs\format.md"'
need '<ComponentRef Id="FormatDoc" />'
test -f docs/format.md
mapfile -t files < <(git ls-files skills/gcad)
[ "${#files[@]}" -gt 0 ] || { echo "no skill files"; exit 1; }
for f in "${files[@]}"; do
  src="Source=\"${f//\//\\}\""
  need "$src"
  id=$(grep -F -B1 -- "$src" "$wxs" | grep -o 'Component Id="[^"]*"' | head -1 | sed 's/Component Id="\(.*\)"/\1/' || true)
  if [ -n "$id" ]; then need "<ComponentRef Id=\"$id\" />"; fi
done
while read -r src; do
  p=${src//\\//}
  printf '%s\n' "${files[@]}" | grep -qxF "$p" || { echo "wxs lists unknown file: $p"; fail=1; }
done < <(grep -o 'Source="skills[^"]*"' "$wxs" | sed 's/Source="\(.*\)"/\1/')
exit $fail
