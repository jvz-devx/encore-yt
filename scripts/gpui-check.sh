#!/usr/bin/env bash
# The full gate (docs/gpui/PLAN.md): verify-workspace
# (fmt, clippy and tests across the workspace, shaders) plus a
# release build of the GPUI app. Run it once before finishing, not while
# iterating.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/dev.sh verify-workspace
printf '\n== release build (app)\n'
cargo build -j "${JOBS:-4}" -p encore-yt --release
printf '\ngpui-check: all passed\n'
