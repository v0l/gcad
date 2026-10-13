#!/bin/sh
set -e
here=$(cd "$(dirname "$0")" && pwd)
case=${1:?cube, sphere, tube or benchmark}
work=${2:-/tmp/gcad-fluidx3d}
precision=${PRECISION:-FP32}
if [ ! -d "$work" ]; then
  git clone -q https://github.com/ProjectPhysX/FluidX3D.git "$work"
  git -C "$work" checkout -q dbf0bb5
fi
cd "$work"
git checkout -q -- src/setup.cpp src/defines.hpp
if [ "$case" != benchmark ]; then
  sed "s/DIAMETER/${DIAMETER:-20}u/; s/NX/${NX:-140}u/; s/NY/${NY:-235}u/; s/NZ/${NZ:-207}u/; s/CELL/${CELL:-5}.0f/; s/SETTLE/${SETTLE:-6327}ull/; s/AVERAGE/${AVERAGE:-22293}ull/; s/CZ/${CZ:-91.5}f/" \
    "$here/$case.cpp" > src/setup.cpp
  sed -i 's|^#define BENCHMARK|//#define BENCHMARK|; s|^//#define FORCE_FIELD|#define FORCE_FIELD|; s|^//#define VOLUME_FORCE|#define VOLUME_FORCE|; s|^//#define EQUILIBRIUM_BOUNDARIES|#define EQUILIBRIUM_BOUNDARIES|' src/defines.hpp
  [ "$case" = sphere ] || sed -i 's|^//#define SUBGRID|#define SUBGRID|' src/defines.hpp
fi
[ "$precision" = FP32 ] && sed -i 's|^#define FP16S|//#define FP16S|' src/defines.hpp
make Linux -j"$(nproc)" > build.log 2>&1
./bin/FluidX3D | tr '\r' '\n' | grep -E "CUBE|SPHERE|TUBE|Peak MLUPs" | tail -1
