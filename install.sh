#!/bin/sh
set -eu

repo=v0l/gcad
bin=${GCAD_BIN:-$HOME/.local/bin}

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  *) echo "no gcad build for $(uname -s) $(uname -m); build it from source" >&2; exit 1 ;;
esac

name=gcad-$target
url=https://github.com/$repo/releases/latest/download/$name.tar.gz
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "downloading $url"
curl -fsSL "$url" -o "$tmp/$name.tar.gz"
curl -fsSL "$url.sha256" -o "$tmp/$name.tar.gz.sha256"
(cd "$tmp" && if command -v sha256sum >/dev/null; then sha256sum -c "$name.tar.gz.sha256"; else shasum -a 256 -c "$name.tar.gz.sha256"; fi) >/dev/null
tar xzf "$tmp/$name.tar.gz" -C "$tmp"

mkdir -p "$bin"
install -m 755 "$tmp/$name/gcad" "$bin/gcad"
echo "installed $("$bin/gcad" --version) to $bin/gcad"
case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "add $bin to your PATH" ;;
esac
