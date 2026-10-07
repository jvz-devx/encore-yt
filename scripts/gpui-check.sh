#!/usr/bin/env bash
# The full gate (docs/gpui/PLAN.md): verify-workspace
# (fmt, clippy and tests across both Cargo workspaces, shaders) plus a
# release build of the GPUI app. Run it once before finishing, not while
# iterating.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/dev.sh verify-workspace
printf '\n== release build (gpui)\n'
(cd gpui && cargo build -j "${JOBS:-4}" --release)
printf '\ngpui-check: all passed\n'
