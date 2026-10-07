#!/usr/bin/env bash
# Crate-scoped Rust commands for fast iteration. The justfile calls this.
#
#   scripts/dev.sh check  CRATE          cargo check of one crate (no tests)
#   scripts/dev.sh test   CRATE [FILTER] that crate's tests, optionally filtered
#   scripts/dev.sh lint   CRATE          clippy -D warnings for that crate
#   scripts/dev.sh verify CRATE          fmt, check, tests and clippy for it
#   scripts/dev.sh verify-workspace      everything, both workspaces (final)
#   scripts/dev.sh shaders               validate every .wgsl with naga
#
# CRATE is one of:
#   gpui      the GPUI app (gpui/, package ytfast-gpui)
#   visuals   the wgpu effects crate (gpui/crates/visuals, ytfast-visuals)
#   audio     the pure Rust playback spike (gpui/crates/audio, ytfast-audio)
#   backend   the root crate without the egui interface (what gpui uses)
#   egui      the root crate with the egui interface (the upstream app)
#
# The root crate and gpui/ are separate Cargo workspaces on purpose (one
# workspace would unify features across both apps; zbus's tokio feature
# breaks AccessKit in the egui app), so "workspace" here means both.
set -euo pipefail
cd "$(dirname "$0")/.."
jobs="${JOBS:-4}"

step() { printf '== %s\n' "$*" >&2; }

# Where a crate lives and how cargo selects it.
crate_dir() {
    case "$1" in
        gpui | visuals | audio) echo gpui ;;
        backend | egui) echo . ;;
        *) echo "unknown crate '$1' (gpui, visuals, audio, backend, egui)" >&2; exit 2 ;;
    esac
}
crate_args() {
    case "$1" in
        gpui) echo "-p ytfast-gpui" ;;
        visuals) echo "-p ytfast-visuals" ;;
        audio) echo "-p ytfast-audio" ;;
        backend) echo "--lib --no-default-features" ;;
        egui) echo "--features e2e" ;;
    esac
}
in_crate() {
    local crate=$1
    shift
    local dir
    dir="$(crate_dir "$crate")"
    (cd "$dir" && "$@")
}

check() {
    step "check $1"
    # shellcheck disable=SC2046
    in_crate "$1" cargo check -j "$jobs" $(crate_args "$1")
}

test_crate() {
    local crate=$1 filter=${2:-}
    case "$crate" in
        # The headless UI tests (gpui/src/ui_tests): GPUI's test platform, no
        # desktop, network or YouTube.
        gpui) step "test gpui"; in_crate gpui cargo test -j "$jobs" -p ytfast-gpui -- $filter ;;
        visuals) step "test visuals"; in_crate gpui cargo test -j "$jobs" -p ytfast-visuals -- $filter ;;
        audio) step "test audio"; in_crate gpui cargo test -j "$jobs" -p ytfast-audio -- $filter ;;
        backend | egui)
            # The parser fixture tests (tests/) and unit tests, without egui.
            step "test backend"
            cargo test -j "$jobs" --lib --tests --no-default-features -- $filter
            ;;
    esac
}

lint() {
    step "clippy $1"
    case "$1" in
        egui) cargo clippy -j "$jobs" --all-targets --features e2e -- -D warnings ;;
        backend) cargo clippy -j "$jobs" --lib --tests --no-default-features -- -D warnings ;;
        *) in_crate "$1" cargo clippy -j "$jobs" $(crate_args "$1") --all-targets -- -D warnings ;;
    esac
}

fmt() {
    step "fmt $1"
    in_crate "$1" cargo fmt --all
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

verify() {
    fmt "$1"
    check "$1"
    test_crate "$1"
    lint "$1"
    if [ "$1" = visuals ]; then shaders; fi
    printf '\nverify %s: passed\n' "$1"
}

verify_workspace() {
    step "fmt (check, both workspaces)"
    cargo fmt --all --check
    (cd gpui && cargo fmt --all --check)
    lint egui
    lint backend
    test_crate backend
    step "clippy gpui workspace"
    (cd gpui && cargo clippy -j "$jobs" --workspace --all-targets -- -D warnings)
    step "test gpui workspace"
    (cd gpui && cargo test -j "$jobs" --workspace)
    shaders
    printf '\nverify-workspace: passed\n'
}

action="${1:-}"
shift || true
case "$action" in
    check) check "${1:-gpui}" ;;
    test) test_crate "${1:-backend}" "${2:-}" ;;
    lint) lint "${1:-gpui}" ;;
    verify) verify "${1:-gpui}" ;;
    verify-workspace) verify_workspace ;;
    shaders) shaders ;;
    *) sed -n '2,20p' "$0"; exit 2 ;;
esac
