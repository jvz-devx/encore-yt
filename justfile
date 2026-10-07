# Fast, crate-scoped commands (scripts/dev.sh does the work).
# Crates: app (the GPUI app), core (the backend library), visuals (wgpu
# effects), audio (the Rust playback engine).

# List the recipes.
default:
    @just --list

# cargo check of one crate: the normal command while iterating.
check crate="app":
    scripts/dev.sh check {{crate}}

# One crate's tests, optionally filtered by name.
test crate="core" filter="":
    scripts/dev.sh test {{crate}} {{filter}}

# clippy -D warnings for one crate.
lint crate="app":
    scripts/dev.sh lint {{crate}}

# Before finishing a task: fmt, check, tests and clippy for one crate.
verify crate="app":
    scripts/dev.sh verify {{crate}}

# Final check across the workspace (fmt, clippy, tests, shaders); heavy.
verify-workspace:
    scripts/dev.sh verify-workspace

# The full gate (verify-workspace plus a release build of the app).
gate:
    scripts/gpui-check.sh

# Validate the WGSL shaders with naga, without building the app.
shaders:
    scripts/dev.sh shaders

# A release-speed build for measurements and effect checks (no LTO).
profiling:
    cargo build -p ytfast-gpui --profile profiling
