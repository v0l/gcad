#!/bin/sh
set -e
here=$(cd "$(dirname "$0")" && pwd)
case=${1:?case directory under tools/foam}
work=${2:-/tmp/gcad-foam/$case}
reynolds=${REYNOLDS:-50}
level=${LEVEL:-4}
near=${NEAR:-3}
wake=${WAKE:-2}
layers=${LAYERS:-true}
iterations=${ITERATIONS:-2000}
procs=${PROCS:-16}
rm -rf "$work"
mkdir -p "$work"
cp -r "$here/$case/." "$work/"
nu=$(python3 -c "print(1.0 / $reynolds)")
sed -i "s/NU/$nu/" "$work/constant/transportProperties"
sed -i "s/ITERATIONS/$iterations/g" "$work/system/controlDict"
sed -i "s/PROCS/$procs/" "$work/system/decomposeParDict"
sed -i "s/LEVEL/$level/g; s/NEAR/$near/; s/WAKE/$wake/; s/LAYERS/$layers/" "$work/system/snappyHexMeshDict"
mkdir -p "$work/constant/triSurface"
cp "${STL:?STL file of the part}" "$work/constant/triSurface/$case.stl"
docker run --rm -v "$work:/case" -w /case opencfd/openfoam-default:2412 bash -lc "cd /case &&
  blockMesh > log.blockMesh 2>&1 &&
  decomposePar -force > log.decompose 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs snappyHexMesh -parallel -overwrite > log.snappy 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs checkMesh -parallel > log.checkMesh 2>&1 &&
  . \$WM_PROJECT_DIR/bin/tools/RunFunctions && restore0Dir -processor > log.restore 2>&1 &&
  mpirun --allow-run-as-root --oversubscribe -np $procs simpleFoam -parallel > log.simpleFoam 2>&1; chown -R $(id -u):$(id -g) /case"
tail -n 1 "$work"/postProcessing/forces/0/coefficient.dat
