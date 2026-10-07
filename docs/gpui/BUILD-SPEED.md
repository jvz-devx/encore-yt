# Build speed for agents

Measured 2026-10-07 on the dev box (6 cores, 15 GB, sccache 0.18, Rust
1.99), with other agents building at the same time, so treat the numbers as
rough. Commands are from the justfile (`scripts/dev.sh`).

## The loop

| What | Command | Time |
|---|---|---|
| No-op check of the app | `just check gpui` | 0.4 s |
| Check after editing a GPUI view | `just check gpui` | 1.0 s |
| Debug build after that edit (to run the app) | `cd gpui && cargo build` | 3.8 s |
| Check the GPUI workspace, all targets | `cd gpui && cargo check --workspace --all-targets` | 2.6 s |
| Check after editing the backend | `just check backend` | 3.6 s |
| The app after that backend edit | `just check gpui` | 1.9 s |
| Backend tests (parser fixtures) | `just test backend` | 3.1 s |
| Visuals tests | `just test visuals` | 0.4 s (54 s the first time) |
| Verify the app before finishing | `just verify gpui` | 4.0 s |
| Release build after one edit (fat LTO since M26, SIZE.md) | `cd gpui && cargo build --release` | 283 s |
| Profiling build after one edit (no LTO) | `just profiling` | 17 s |

## New worktrees

| What | Time | sccache Rust hits |
|---|---|---|
| Fresh worktree, full debug build, before `basedirs` | 1395 s | 52% |
| Fresh worktree, `just check gpui`, with `basedirs` | 122–146 s | 100% |
| First profiling build in a target dir | 610 s | |

`basedirs` in ~/.config/sccache/config strips the main checkout and the
worktree slots (`yt-rust-wt/slot1..8`) from cache keys, so a new slot reuses
what any other checkout compiled. What's left of a cold check is work sccache
can't cache: proc-macro crates (gpui's derives, serde, zbus), build scripts,
and cargo itself. Hence the rule to reuse slots (`git checkout -B <branch>
main`) instead of making new worktrees: a warm slot skips all of it.

## Remaining bottlenecks, and what's not worth doing now

- **Cold target directories** (2–10 min): mostly uncacheable proc-macros and
  build scripts. Fixed by keeping slot worktrees warm, not by code changes.
- **Release builds** (~3 min per edit): thin LTO over ~860 crates. Kept for
  CI and packaging only; measurements use the `profiling` profile.
- **Dependencies at opt-level 2 in dev** make cold builds slower, but GPUI
  is too slow to use at opt-level 0. Kept.
- **One 17k-line `ytfast-gpui` crate**: an edit checks in ~1 s, so splitting
  it would gain little. Not worth it now.
- **Two Cargo workspaces** (the root crate and `gpui/`): kept apart while
  the egui app lived in the root crate, because unifying features would have
  turned on zbus's tokio feature for it and broken its AccessKit. That app is
  gone (M23), so the reason is too; one workspace is planned in M23.
- **cargo-nextest**: 16 tests that run in under a second; no gain.
- **mold**: Rust 1.99 already links with rust-lld on Linux; a debug build
  after an edit is under 4 s in total.
- **GitHub CI** (~20 min per release run) uses rust-cache and runs no tests;
  it runs in the background and doesn't block agents.

## Release builds skip sccache (2026-10-07)

Full release builds through the shared sccache server took 20 to 60 minutes
during M26, and about 7 without it (fat LTO and per-crate opt-levels rarely
hit the cache anyway). `scripts/rustc-wrapper` now calls rustc directly for
anything under `target/…/release/`; checks, dev and `profiling` builds still
go through sccache. CI already builds without it.
