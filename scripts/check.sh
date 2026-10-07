#!/usr/bin/env bash
# Kept for older docs and habits; scripts/dev.sh (and the justfile) is the
# interface now. Each command checks only what you name.
#
#   scripts/check.sh gpui|backend|egui|visuals   cargo check of that crate
#   scripts/check.sh tests                        backend tests (parser fixtures)
#   scripts/check.sh shaders                      validate .wgsl with naga
set -euo pipefail
dev="$(dirname "$0")/dev.sh"
case "${1:-}" in
    gpui | backend | egui) exec "$dev" check "$1" ;;
    visuals) "$dev" check visuals && exec "$dev" test visuals ;;
    tests) exec "$dev" test backend ;;
    shaders) exec "$dev" shaders ;;
    *) sed -n '2,7p' "$0"; exit 2 ;;
esac
