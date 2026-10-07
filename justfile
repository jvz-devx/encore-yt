# Fast, crate-scoped commands (scripts/dev.sh does the work).
# Crates: gpui (the GPUI app), visuals (wgpu effects), audio (the playback
# spike), backend (root crate without egui), egui (root crate with the egui
# app).

# List the recipes.
default:
    @just --list

# cargo check of one crate: the normal command while iterating.
check crate="gpui":
    scripts/dev.sh check {{crate}}

# One crate's tests, optionally filtered by name.
test crate="backend" filter="":
    scripts/dev.sh test {{crate}} {{filter}}

# clippy -D warnings for one crate.
lint crate="gpui":
    scripts/dev.sh lint {{crate}}

# Before finishing a task: fmt, check, tests and clippy for one crate.
verify crate="gpui":
    scripts/dev.sh verify {{crate}}

# Final check across both workspaces (fmt, clippy, tests, shaders); heavy.
verify-workspace:
    scripts/dev.sh verify-workspace

# The full gate (verify-workspace plus a release build of the GPUI app).
gate:
    scripts/gpui-check.sh

# Validate the WGSL shaders with naga, without building the app.
shaders:
    scripts/dev.sh shaders

# A release-speed build for measurements and effect checks (no LTO).
profiling:
    cd gpui && cargo build --profile profiling
