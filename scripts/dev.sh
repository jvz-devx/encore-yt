#!/usr/bin/env bash
# Crate-scoped Rust commands for fast iteration. The justfile calls this.
#
#   scripts/dev.sh check  CRATE          cargo check of one crate (no tests)
#   scripts/dev.sh test   CRATE [FILTER] that crate's tests, optionally filtered
#   scripts/dev.sh lint   CRATE          clippy -D warnings for that crate
#   scripts/dev.sh verify CRATE          fmt, check, tests and clippy for it
#   scripts/dev.sh verify-workspace      everything in the workspace (final)
#   scripts/dev.sh shaders               validate every .wgsl with naga
#
# CRATE is one of:
#   app       the GPUI app (crates/app, package encore-yt)
#   core      the backend library the app uses (crates/core, package encore-core)
#   visuals   the wgpu effects crate (crates/visuals, encore-visuals)
#   audio     the Rust playback engine (crates/audio, encore-audio)
#   signin    the sign-in window helper (crates/signin, encore-signin)
#   cast      the casting spike (crates/cast, encore-cast)
set -euo pipefail
cd "$(dirname "$0")/.."
jobs="${JOBS:-4}"

step() { printf '== %s\n' "$*" >&2; }

# The package cargo selects for a crate.
package() {
    case "$1" in
        app) echo encore-yt ;;
        core) echo encore-core ;;
        visuals) echo encore-visuals ;;
        audio) echo encore-audio ;;
        signin) echo encore-signin ;;
        cast) echo encore-cast ;;
        *) echo "unknown crate '$1' (app, core, visuals, audio, signin, cast)" >&2; exit 2 ;;
    esac
}

check() {
    local pkg
    pkg="$(package "$1")"
    step "check $1"
    cargo check -j "$jobs" -p "$pkg"
}

test_crate() {
    local crate=$1 filter=${2:-} pkg
    pkg="$(package "$crate")"
    step "test $crate"
    # app: the headless UI tests (crates/app/src/ui_tests), GPUI's test
    # platform with no desktop, network or YouTube. core: the parser
    # fixture tests (crates/core/tests) and unit tests.
    # shellcheck disable=SC2086
    case "$crate" in
        core) cargo test -j "$jobs" -p "$pkg" --lib --tests -- $filter ;;
        *) cargo test -j "$jobs" -p "$pkg" -- $filter ;;
    esac
}

lint() {
    local pkg
    pkg="$(package "$1")"
    step "clippy $1"
    cargo clippy -j "$jobs" -p "$pkg" --all-targets -- -D warnings
}

fmt() {
    local pkg
    pkg="$(package "$1")"
    step "fmt $1"
    cargo fmt -p "$pkg"
}

shaders() {
    step "validate shaders"
    local found=0 file joined
    while IFS= read -r -d '' file; do
        found=1
        case "$file" in
            # The 3D scenes compile after their shared part (scene.rs).
            */scene_*.wgsl)
                [ "${file##*/}" = scene_common.wgsl ] && continue
                joined="$(mktemp -t scene.XXXXXX).wgsl"
                cat "$(dirname "$file")/scene_common.wgsl" "$file" >"$joined"
                naga "$joined" >/dev/null && echo "ok  $file"
                rm -f "$joined"
                ;;
            *) naga "$file" >/dev/null && echo "ok  $file" ;;
        esac
    done < <(find crates -name '*.wgsl' -print0)
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
    step "fmt (check)"
    cargo fmt --all --check
    step "clippy (workspace)"
    cargo clippy -j "$jobs" --workspace --all-targets -- -D warnings
    step "test (workspace)"
    cargo test -j "$jobs" --workspace --lib --bins --tests
    shaders
    printf '\nverify-workspace: passed\n'
}

action="${1:-}"
shift || true
case "$action" in
    check) check "${1:-app}" ;;
    test) test_crate "${1:-core}" "${2:-}" ;;
    lint) lint "${1:-app}" ;;
    verify) verify "${1:-app}" ;;
    verify-workspace) verify_workspace ;;
    shaders) shaders ;;
    *) sed -n '2,15p' "$0"; exit 2 ;;
esac
