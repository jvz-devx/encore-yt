@AGENTS.md

## Rust build discipline

Keep feedback loops fast. Group related edits, then use `just check <crate>` (`cargo check -p <affected-crate>`) and targeted tests (`just test <crate> [filter]`). Never `cargo build` just to validate, and don't run workspace-wide checks, clippy or tests after every edit. Before finishing, `just verify <crate>` for each crate you touched; `just verify-workspace` or `just gate` only at the end or when the change spans several crates. Preserve caches, never run `cargo clean` unless required. AGENTS.md "Rust builds" has the details.
