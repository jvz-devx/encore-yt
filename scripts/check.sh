#!/usr/bin/env bash
# Fast, targeted checks while iterating. Check what you touched, then run
# scripts/gpui-check.sh once before finishing.
#
#   scripts/check.sh gpui      cargo check of the GPUI app (and the backend it uses)
#   scripts/check.sh backend   cargo check of the backend alone (no egui)
#   scripts/check.sh egui      cargo check of the egui app, all targets
#   scripts/check.sh tests     backend tests, including the parser fixtures
#   scripts/check.sh shaders   validate every .wgsl under gpui/ with naga
#   scripts/check.sh all       everything above (still no release build)
#
# The root crate and gpui/ are separate Cargo workspaces on purpose: one
# workspace would unify features across both apps (zbus's tokio feature
# breaks AccessKit in the egui app; see Cargo.toml), so `--workspace` is
# not used here. Builds go through sccache when ~/.cargo/config.toml sets
# it as the rustc wrapper; each worktree keeps its own target/.
set -euo pipefail
cd "$(dirname "$0")/.."
jobs="${JOBS:-4}"

step() { printf '== %s\n' "$*"; }

gpui() {
    step "check gpui"
    (cd gpui && cargo check -j "$jobs" -p ytfast-gpui --all-targets)
}
backend() {
    step "check backend"
    cargo check -j "$jobs" --lib --tests --no-default-features
}
egui() {
    step "check egui app"
    cargo check -j "$jobs" --all-targets --features e2e
}
tests() {
    step "test backend"
    cargo test -j "$jobs" --lib --tests --no-default-features
}
shaders() {
    step "validate shaders"
    local found=0 file
    while IFS= read -r -d '' file; do
        found=1
        naga "$file" >/dev/null && echo "ok  $file"
    done < <(find gpui -name '*.wgsl' -not -path '*/target/*' -print0)
    [ "$found" = 1 ] || echo "no .wgsl files"
}

case "${1:-}" in
    gpui) gpui ;;
    backend) backend ;;
    egui) egui ;;
    tests) tests ;;
    shaders) shaders ;;
    all) backend; tests; gpui; egui; shaders ;;
    *) sed -n '2,10p' "$0"; exit 2 ;;
esac
