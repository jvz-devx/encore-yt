#!/usr/bin/env bash
# The gate for the GPUI fork (docs/gpui/PLAN.md): formatting, lints for the
# root crate with and without the egui interface, the GPUI app, and a
# release build of the GPUI app. Exits non-zero on the first failure.
set -euo pipefail
cd "$(dirname "$0")/.."
jobs="${JOBS:-4}"

step() { printf '\n== %s\n' "$*"; }

step "fmt (root)"
cargo fmt --all --check
step "fmt (gpui)"
(cd gpui && cargo fmt --all --check)

step "clippy (root, egui)"
cargo clippy -j "$jobs" --all-targets --features e2e -- -D warnings
step "clippy (root, backend only)"
cargo clippy -j "$jobs" --lib --no-default-features -- -D warnings
step "tests (root, backend only)"
cargo test -j "$jobs" --lib --tests --no-default-features

step "clippy (gpui)"
(cd gpui && cargo clippy -j "$jobs" --all-targets -- -D warnings)
step "tests (gpui)"
(cd gpui && cargo test -j "$jobs")
step "release build (gpui)"
(cd gpui && cargo build -j "$jobs" --release)

printf '\ngpui-check: all passed\n'
