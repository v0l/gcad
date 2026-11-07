#!/usr/bin/env bash
set -uo pipefail
bin=$(realpath "$1")
out=$2
mode=${3:-default}
root=$(cd "$(dirname "$0")/.." && pwd)
work=$(mktemp -d /tmp/gcad-baseline.XXXX)
trap 'rm -rf "$work"' EXIT
cd "$root"
export RAYON_NUM_THREADS=${RAYON_NUM_THREADS:-4}
jobs=${BASELINE_JOBS:-6}

task() {
    local bin=$1 work=$2 kind=$3 file=$4
    local key; key=$(echo "$kind $file" | tr '/ ' '__')
    local dir=$work/$key
    mkdir -p "$dir"
    local status=0
    case $kind in
        check) timeout 600 "$bin" check "$file" >"$dir/out" 2>"$dir/err" || status=$? ;;
        dry) timeout 600 "$bin" sim --dry-run --quiet "$file" >"$dir/out" 2>"$dir/err" || status=$? ;;
        job)
            mkdir -p "$dir/result"
            timeout 600 "$bin" sim-job --quiet "$file" "$dir/result" >"$dir/out" 2>"$dir/err" || status=$? ;;
        render) timeout 600 "$bin" render "$file" "$dir/x.png" >"$dir/out" 2>"$dir/err" || status=$? ;;
        bom) timeout 600 "$bin" bom "$file" >"$dir/out" 2>"$dir/err" || status=$? ;;
        *) timeout 600 "$bin" export "$file" "$dir/x.$kind" >"$dir/out" 2>"$dir/err" || status=$? ;;
    esac
    local files
    files=$(cd "$dir" && find . -type f ! -name err -printf '%P\n' | sort)
    local sum
    sum=$(cd "$dir" && { echo "status $status"; for f in $files; do echo "$f"; normal "$f" "$dir" | sha256sum; done; } | sha256sum | cut -c1-16)
    echo "$sum $status $kind $file"
}
normal() {
    case $1 in
        *.step) sed -E "s/^FILE_NAME\('', '[^']*'/FILE_NAME('', ''/" "$1" ;;
        *.svg) LC_ALL=C sort "$1" ;;
        out) sed -E -e "s#$2#DIR#g" -e 's/[0-9]+(\.[0-9]+)? (s|ms)\b/T \2/g' "$1" ;;
        *) cat "$1" ;;
    esac
}
export -f task normal

{
    for f in $(find examples tests/fixtures -name '*.gcad' -o -name '*.gasm' | sort); do
        for k in check stl obj step 3mf svg dxf glb render; do echo "$k $f"; done
    done
    for f in $(find examples tests/fixtures -name '*.gasm' | sort); do echo "bom $f"; done
    for f in $(find examples -name '*.gsim' | sort); do echo "dry $f"; done
    for f in $(find tests/fixtures/jobs -name '*.job' | sort); do echo "job $f"; done
    if [ "$mode" = game-assets ]; then
        for f in $(find examples tests/fixtures -name '*.gasset' | sort); do
            for k in check glb; do echo "$k $f"; done
        done
    fi
} | xargs -P "$jobs" -L 1 bash -c 'task "$0" "$1" "$2" "$3"' "$bin" "$work" |
    sort -k4,4 -k3,3 >"$out"
wc -l <"$out"
