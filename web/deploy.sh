#!/bin/sh
set -e
cd "$(dirname "$0")/.."
./web/build.sh
bunx wrangler pages deploy target/web --project-name gcad --branch master --commit-dirty=true
