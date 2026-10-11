#!/bin/sh
set -e
here=$(cd "$(dirname "$0")" && pwd)
work=${1:?work directory}
stl=${STL:?STL of the parts in mm, from gcad export flow.gsim parts.stl}
toward=${TOWARD:-1,0,0}
speed=${SPEED:-10}
across=${ACROSS:?square root of the frontal area in mm, from gcad sim flow.gsim --dry-run}
cells=${CELLS:-8}
low=${LOW:-3}
high=${HIGH:-4}
near=${NEAR:-2}
iterations=${ITERATIONS:-1500}
procs=${PROCS:-40}
density=${DENSITY:-1.204}
viscosity=${VISCOSITY:-1.516e-5}
rm -rf "$work"
mkdir -p "$work/constant/triSurface"
cp -r "$here/wind/." "$work/"
bounds=$(python3 "$here/wind.py" "$stl" "$work/constant/triSurface/part.stl" "$toward")
eval "$(python3 - "$bounds" "$across" "$cells" "$speed" <<'PY'
import sys
b = [float(v) for v in sys.argv[1].split()]
h, cells, speed = float(sys.argv[2]) * 0.001, int(sys.argv[3]), float(sys.argv[4])
low, high = b[:3], b[3:]
x0, x1 = low[0] - 4 * h, high[0] + 5 * h
y0, y1 = low[1] - 3 * h, high[1] + 3 * h
z0, z1 = low[2] - 3 * h, high[2] + 3 * h
size = h / cells
n = [max(1, round((x1 - x0) / size)), max(1, round((y1 - y0) / size)), max(1, round((z1 - z0) / size))]
k = 1.5 * (0.01 * speed) ** 2
omega = k ** 0.5 / (0.09 ** 0.25 * 0.1 * h)
pad = 0.5 * h
print(f"X0={x0} X1={x1} Y0={y0} Y1={y1} Z0={z0} Z1={z1} NX={n[0]} NY={n[1]} NZ={n[2]}")
print(f"KIN={k} OMEGA={omega}")
print(f"NEARMIN='{low[0]-pad} {low[1]-pad} {low[2]-pad}' NEARMAX='{high[0]+2*pad} {high[1]+pad} {high[2]+pad}'")
print(f"INSIDE='{x0 + 0.37 * h} {y0 + 0.41 * h} {z0 + 0.43 * h}'")
PY
)"
sed -i "s/X0/$X0/g; s/X1/$X1/g; s/Y0/$Y0/g; s/Y1/$Y1/g; s/Z0/$Z0/g; s/Z1/$Z1/g; s/NX/$NX/; s/NY/$NY/; s/NZ/$NZ/" "$work/system/blockMeshDict"
sed -i "s/NEARMIN/$NEARMIN/; s/NEARMAX/$NEARMAX/; s/INSIDE/$INSIDE/; s/LOW/$low/; s/HIGH/$high/; s/NEAR)/$near)/" "$work/system/snappyHexMeshDict"
sed -i "s/ITERATIONS/$iterations/g; s/DENSITY/$density/" "$work/system/controlDict"
sed -i "s/PROCS/$procs/" "$work/system/decomposeParDict"
sed -i "s/VISCOSITY/$viscosity/" "$work/constant/transportProperties"
sed -i "s/SPEED/$speed/g" "$work/0.orig/U"
sed -i "s/KIN/$KIN/g" "$work/0.orig/k"
sed -i "s/OMEGA/$OMEGA/g" "$work/0.orig/omega"
docker run --rm -v "$work:/case" opencfd/openfoam-default:2412 bash -lc "cd /case &&
  . \$WM_PROJECT_DIR/bin/tools/RunFunctions &&
  blockMesh > log.blockMesh 2>&1 &&
  decomposePar -force > log.decompose 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs snappyHexMesh -parallel -overwrite > log.snappy 2>&1 &&
  restore0Dir -processor > log.restore 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs checkMesh -parallel > log.checkMesh 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs simpleFoam -parallel > log.simpleFoam 2>&1; chown -R $(id -u):$(id -g) /case"
tail -n 1 "$work"/postProcessing/forces/0/force.dat
