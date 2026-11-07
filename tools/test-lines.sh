#!/usr/bin/env bash
awk '
/^ +Running /{ split($NF, p, "/"); bin=p[length(p)]; sub(/-[0-9a-f]+\)$/, "", bin); name=$2" "bin }
/^ +Doc-tests /{ name="doc "$2 }
/^test result/{ line=$0; sub(/; finished.*/, "", line); sub(/^test result: /, "", line); print name": "line }
' "$1" | sort
