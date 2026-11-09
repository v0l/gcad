#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run --release -p gcad-cloud --example fixtures -- tests/fixtures/clouds
ls -l tests/fixtures/clouds
