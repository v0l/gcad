#!/bin/sh
# Development entry points for the optional game-asset pipeline.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
fail() { printf '%s\n' "$*" >&2; exit 1; }
usage() {
    cat <<'EOF'
Usage: sh tools/game-assets.sh COMMAND [ARGS]
  prepare-fixtures                         Preserve/create bindings, check fixtures

Use GCAD to select a built gcad executable.
EOF
}
command=${1:---help}
[ "$#" -eq 0 ] || shift
case "$command" in
--help|-h) usage ;;
prepare-fixtures)
    [ "$#" -eq 0 ] || fail 'prepare-fixtures takes no arguments; use GCAD for the executable'
    gcad=${GCAD:-"$root/target/debug/gcad"}
    [ -f "$gcad" ] || gcad="$gcad.exe"
    [ -f "$gcad" ] || fail 'Build gcad with --features game-assets, or set GCAD'
    mkdir -p target
    while IFS='|' read -r selection filename; do
        [ -e "target/$filename" ] && continue
        "$gcad" asset-face-bind examples/game-assets/plate-face-materials.gasset plate "$selection" "target/$filename"
    done <<'EOF'
base.end|game-asset-plate-end.faces.json
<Z|game-asset-plate-bottom.faces.json
EOF
    while IFS='|' read -r selection filename; do
        [ -e "target/$filename" ] && continue
        "$gcad" asset-face-bind examples/game-assets/plate-scene-refined.gasset plate "$selection" "target/$filename"
    done <<'EOF'
base.end|game-asset-refined-end.faces.json
<Z|game-asset-refined-bottom.faces.json
EOF
    while IFS='|' read -r fixture node filename joints option value; do
        [ -e "target/$filename" ] && continue
        set -- asset-bind "examples/game-assets/$fixture.gasset" "$node" "target/$filename" --joints "$joints"
        [ -z "$option" ] || set -- "$@" "$option" "$value"
        "$gcad" "$@"
    done <<'EOF'
nested-skinned|first/left.a|game-asset-nested.weights.json|bone||
linkage-part-skin|first/crank|game-asset-linkage-part.weights.json|bone||
plate-skinned|plate|game-asset-plate.v2.weights.json|base,tip|--blend|y,-15,15
plate-root-motion-skinned|plate|root-motion-skinned.weights.json|base,tip|--blend|y,-15,15
plate-scene-contract|plate|game-asset-plate-scene-contract.weights.json|base,tip|--blend|y,-15,15
plate-scene-refined|plate|game-asset-plate-scene-refined.weights.json|base,tip|--blend|y,-15,15
plate-refined|plate|game-asset-plate-refined.weights.json|base,tip|--blend|y,-15,15
plate-flex|plate|game-asset-plate-flex.weights.json|base,tip|--blend|y,-15,15
plate-flex-narrow|plate|game-asset-plate-flex-narrow.weights.json|base,tip|--blend|y,-7.5,7.5
plate-attachments|plate|game-asset-plate-attachments.weights.json|base,tip|--blend|y,-15,15
plate-twist-chain|plate|game-asset-plate-twist-chain.weights.json|bone0,bone1,bone2,bone3,bone4|--chain|y,-15,-7.5,0,7.5,15
plate-twist-sliced|plate|game-asset-plate-twist-sliced.weights.json|bone0,bone1,bone2,bone3,bone4|--chain|y,-15,-7.5,0,7.5,15
plate-twist-sliced-fine|plate|game-asset-plate-twist-sliced-fine.weights.json|bone0,bone1,bone2,bone3,bone4|--chain|y,-15,-7.5,0,7.5,15
EOF
    for level in high low; do
        output="target/consumer-skinned-$level.weights.json"
        [ -e "$output" ] || "$gcad" asset-bind examples/game-assets/plate-consumer-skinned.gasset "detail_$level" "$output" --joints base,tip --blend y,-15,15
    done
    for fixture in examples/game-assets/*.gasset; do "$gcad" check "$fixture"; done
    ;;
*) usage >&2; fail "Unknown command: $command";;
esac
