@AGENTS.md

## Rust build discipline

Keep feedback loops fast. Group related edits, then use `cargo check -p <affected-crate>` and targeted tests. Do not run full builds/workspace tests after every edit. Preserve caches, never run `cargo clean` unless required, and only do a full workspace validation before finishing or when the change genuinely spans the workspace.
