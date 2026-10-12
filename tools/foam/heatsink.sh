#!/bin/sh
set -e
here=$(cd "$(dirname "$0")" && pwd)
work=${1:?work directory}
cell=${CELL:-0.5}
speed=${SPEED:-2}
watts=${WATTS:-10}
iterations=${ITERATIONS:-3000}
procs=${PROCS:-40}
rm -rf "$work"
mkdir -p "$work"
cp -r "$here/heatsink/." "$work/"
eval "$(python3 - "$cell" "$work" <<'PY'
import sys
cell, work = float(sys.argv[1]), sys.argv[2]
x0, x1 = -123.0, 204.0
print(f"X0={x0} X1={x1} NX={round((x1 - x0) / cell)} NY={round(40 / cell)} NZ={round(25 / cell)}")
fins = "\n".join(
    f"    {{ name sinkCells; type cellSet; action add; source boxToCell; box (0 {(-20 + i * 5.5) / 1000} 0.005) (0.05 {(-20 + i * 5.5 + 1.5) / 1000} 0.025); }}"
    for i in range(8)
)
open(f"{work}/fins.txt", "w").write(fins)
PY
)"
sed -i "s/X0/$X0/g; s/X1/$X1/g; s/NX/$NX/; s/NY/$NY/; s/NZ/$NZ/" "$work/system/blockMeshDict"
python3 - "$work" <<'PY'
import sys
work = sys.argv[1]
path = f"{work}/system/topoSetDict"
text = open(path).read().replace("    FINS\n", open(f"{work}/fins.txt").read() + "\n")
open(path, "w").write(text)
PY
rm -f "$work/fins.txt"
flux=$(python3 -c "print($watts / (0.04 * 0.05))")
sed -i "s/SPEED/$speed/g" "$work/system/air/changeDictionaryDict"
sed -i "s/FLUX/$flux/" "$work/system/sink/changeDictionaryDict"
sed -i "s/ITERATIONS/$iterations/g" "$work/system/controlDict"
sed -i "s/PROCS/$procs/" "$work/system/decomposeParDict"
cp "$work/system/decomposeParDict" "$work/system/air/"
cp "$work/system/decomposeParDict" "$work/system/sink/"
docker run --rm --shm-size=16g -v "$work:/case" -w /case opencfd/openfoam-default:2412 bash -lc "cd /case &&
  . \$WM_PROJECT_DIR/bin/tools/RunFunctions &&
  blockMesh > log.blockMesh 2>&1 &&
  topoSet > log.topoSet 2>&1 &&
  restore0Dir > log.restore 2>&1 &&
  splitMeshRegions -cellZones -overwrite > log.split 2>&1 &&
  rm -f 0/sink/U 0/sink/p_rgh &&
  changeDictionary -region air > log.change.air 2>&1 &&
  changeDictionary -region sink > log.change.sink 2>&1 &&
  decomposePar -allRegions > log.decompose 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs chtMultiRegionSimpleFoam -parallel > log.solve 2>&1;
  chown -R $(id -u):$(id -g) /case"
base=$(grep "areaAverage(floor) of T" "$work/log.solve" | tail -1 | awk '{print $NF}')
[ -n "$base" ] || { echo "no base temperature in $work/log.solve (it is written every 50 iterations)"; exit 1; }
python3 -c "t = $base; print(f'base {t - 273.15:.3f} C, resistance {(t - 298.15) / $watts:.4f} K/W')"
