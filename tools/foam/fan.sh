#!/bin/sh
set -e
here=$(cd "$(dirname "$0")" && pwd)
work=${1:?work directory}
stl=${STL:?STL of the rotor in mm, centred on its axis, the axis along z}
rpm=${RPM:-3000}
across=${ACROSS:?square root of the frontal area in mm, from gcad sim fan.gsim --dry-run}
cells=${CELLS:-8}
low=${LOW:-4}
high=${HIGH:-5}
near=${NEAR:-3}
wake=${WAKE:-2}
iterations=${ITERATIONS:-3000}
procs=${PROCS:-40}
density=${DENSITY:-1.204}
viscosity=${VISCOSITY:-1.516e-5}
rm -rf "$work"
mkdir -p "$work/constant/triSurface"
cp -r "$here/fan/." "$work/"
eval "$(python3 - "$stl" "$work/constant/triSurface/rotor.stl" "$across" "$cells" "$rpm" <<'PY'
import math, struct, sys
source, target = sys.argv[1], sys.argv[2]
h, cells, rpm = float(sys.argv[3]) * 0.001, int(sys.argv[4]), float(sys.argv[5])
data = open(source, "rb").read()
count = struct.unpack_from("<I", data, 80)[0]
low, high = [math.inf] * 3, [-math.inf] * 3
out = bytearray(data[:84])
reach = 0.0
for t in range(count):
    values = list(struct.unpack_from("<12f", data, 84 + 50 * t))
    for k in range(1, 4):
        p = [v * 0.001 for v in values[3 * k : 3 * k + 3]]
        values[3 * k : 3 * k + 3] = p
        reach = max(reach, math.hypot(p[0], p[1]))
        for i in range(3):
            low[i] = min(low[i], p[i])
            high[i] = max(high[i], p[i])
    out += struct.pack("<12fH", *values, 0)
open(target, "wb").write(out)
x0, x1 = low[0] - 3 * h, high[0] + 3 * h
y0, y1 = low[1] - 3 * h, high[1] + 3 * h
z0, z1 = low[2] - 4.5 * h, high[2] + 4.5 * h
size = h / cells
n = [max(1, round((b - a) / size)) for a, b in ((x0, x1), (y0, y1), (z0, z1))]
tip = rpm * 2 * math.pi / 60 * reach
k = 1.5 * (0.01 * tip) ** 2
omega = k ** 0.5 / (0.09 ** 0.25 * 0.1 * h)
gap = 0.5 * (high[2] - low[2])
print(f"X0={x0} X1={x1} Y0={y0} Y1={y1} Z0={z0} Z1={z1} NX={n[0]} NY={n[1]} NZ={n[2]}")
print(f"KIN={k} OMEGA={omega} OMEGA_RAD={rpm * 2 * math.pi / 60}")
print(f"ZLOW={low[2] - gap} ZHIGH={high[2] + gap} RADIUS={reach * 1.025}")
print(f"WAKELOW={low[2] - 2 * h} WAKEHIGH={high[2] + 2 * h} WAKERADIUS={reach * 1.3}")
print(f"INSIDE='{x0 + 0.37 * h} {y0 + 0.41 * h} {z0 + 0.43 * h}'")
PY
)"
sed -i "s/X0/$X0/g; s/X1/$X1/g; s/Y0/$Y0/g; s/Y1/$Y1/g; s/Z0/$Z0/g; s/Z1/$Z1/g; s/NX/$NX/; s/NY/$NY/; s/NZ/$NZ/" "$work/system/blockMeshDict"
sed -i "s/ZLOW/$ZLOW/; s/ZHIGH/$ZHIGH/; s/WAKERADIUS/$WAKERADIUS/; s/RADIUS/$RADIUS/; s/WAKELOW/$WAKELOW/; s/WAKEHIGH/$WAKEHIGH/; s/INSIDE/$INSIDE/; s/LOW HIGH/$low $high/; s/NEAR NEAR/$near $near/; s/1E15 NEAR/1E15 $near/; s/1E15 WAKE/1E15 $wake/" "$work/system/snappyHexMeshDict"
sed -i "s/ITERATIONS/$iterations/g; s/DENSITY/$density/" "$work/system/controlDict"
sed -i "s/PROCS/$procs/" "$work/system/decomposeParDict"
sed -i "s/VISCOSITY/$viscosity/" "$work/constant/transportProperties"
sed -i "s/OMEGA_RAD/$OMEGA_RAD/" "$work/constant/MRFProperties"
sed -i "s/KIN/$KIN/g" "$work/0.orig/k"
sed -i "s/OMEGA/$OMEGA/g" "$work/0.orig/omega"
docker run --rm --shm-size=16g -v "$work:/case" opencfd/openfoam-default:2412 bash -lc "cd /case &&
  . \$WM_PROJECT_DIR/bin/tools/RunFunctions &&
  blockMesh > log.blockMesh 2>&1 &&
  decomposePar -force > log.decompose 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs snappyHexMesh -parallel -overwrite > log.snappy 2>&1 &&
  restore0Dir -processor > log.restore 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs checkMesh -parallel > log.checkMesh 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs simpleFoam -parallel > log.simpleFoam 2>&1; chown -R $(id -u):$(id -g) /case"
tail -n 1 "$work"/postProcessing/forces/0/force.dat
tail -n 1 "$work"/postProcessing/forces/0/moment.dat
tail -n 1 "$work"/postProcessing/disc/0/surfaceFieldValue.dat
