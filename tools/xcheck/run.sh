#!/bin/sh
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=${1:-/tmp/gcad-xcheck}
mkdir -p "$work"
cd "$work"
[ -x .venv/bin/python ] || { uv venv -q .venv && uv pip install -q --python .venv/bin/python gmsh scikit-fem meshio scipy numpy pypardiso; }
cp "$root"/examples/parts/bracket.gcad "$root"/examples/parts/heatsink.gcad "$here"/*.gcad "$here"/joint.gasm "$here"/*.py .
gcad=${GCAD:-"$root/target/release/gcad"}
for f in bracket heatsink col angle; do "$gcad" export $f.gcad $f.step; done
"$gcad" export joint.gasm joint.step
timeout 300 .venv/bin/python reference.py static 2
timeout 300 .venv/bin/python reference.py modal 2
timeout 300 .venv/bin/python reference.py heat 2
timeout 300 .venv/bin/python joint.py 2
timeout 300 .venv/bin/python buckle.py col.step 3 200
timeout 300 .venv/bin/python buckle.py angle.step 2 150
