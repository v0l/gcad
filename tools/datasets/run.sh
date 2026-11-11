#!/bin/sh
cd "$(dirname "$0")"
PY=${GCAD_DATA:-/home/kieran/gcad-datasets}/venv/bin/python
LOG=${GCAD_DATA:-/home/kieran/gcad-datasets}/raw
printf '%s\n' "$@" | xargs -P 2 -I{} sh -c "nice -n 19 ionice -c3 $PY -u {}.py > $LOG/run-{}.log 2>&1"
