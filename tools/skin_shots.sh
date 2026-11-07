#!/bin/sh
set -e
gcad=${GCAD:-gcad}
out=${1:-/tmp/skin-shots}
root=$(dirname "$0")/..
mkdir -p "$out"
shot() {
    name=$1 clip=$2 t=$3 view=$4 zoom=$5
    file="$out/$name-$clip-$t.gcad"
    cp "$root/examples/mesh/$name.gcad" "$file"
    echo "pose clip=$clip t=$t" >>"$file"
    "$gcad" inspect "$file" "$out/$name-$clip-$t-$view.png" --view "$view" "--zoom=$zoom" >/dev/null
}
shot rigged_creature walk 0.5 left 0,6,7,6
shot rigged_creature walk 0.5 back 0,6,7,6
shot rigged_creature walk 0 left 0,6,7,6
shot skin_stress rom 1 left 0,6,7,7
shot skin_stress rom 1 back 0,6,7,7
shot skin_arm rom 1 right 0,-4,6,6
shot skin_arm rom 2 front 0,0,22,5
ls "$out"/*.png
