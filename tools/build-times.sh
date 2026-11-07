#!/usr/bin/env bash
set -u
for f in "$@"; do
    touch "$f"
    s=$(date +%s.%N)
    nice -n 15 ionice -c3 cargo build --release --workspace -q 2>/dev/null
    printf '%6.1f s  %s\n' "$(echo "$(date +%s.%N) - $s" | bc)" "$f"
done
