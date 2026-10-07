# Encore agent guide

Encore (encore-yt) is a native YouTube Music client for Linux, Windows and macOS, written in Rust with [GPUI](https://www.gpui.rs). It has no browser engine, no telemetry and no server of its own. Shown to people as "Encore"; `encore-yt` is the command, package and folder name, `io.github.jvz-devx.encore-yt` the app id. The repository is jvz-devx/encore-yt; it started as ytfast-gpui, from ytfast (see README), and nothing is merged from there any more.

## The project

- One Cargo workspace (one `Cargo.lock`, one `target/`): `crates/core` (`encore-core`, a library) is the backend: InnerTube, sign-in, playback, the queue and the desktop services (MPRIS, tray, notifications). `crates/app` is the app (`encore-yt`), `crates/visuals` its wgpu effects and `crates/audio` the Rust playback engine. Installer files are in `packaging/`.
- [docs/gpui/PLAN.md](docs/gpui/PLAN.md) holds the milestones, how each is verified, and the log. Read it first.
- Main platform: Fedora, KDE Plasma on Wayland. Colours, radii, spacing and type come from the app's theme module (`crates/app/src/theme.rs`), never hard-coded in views.
- Verify UI work visually: run the app on the Wayland session and capture it (see PLAN.md). Don't claim a view works without looking at a capture.
- Commit in small topical commits on `main` and push to `origin`.
- After each release, the release workflow commits the updated Homebrew cask (`Casks/encore-yt.rb`) to `main`, so pull before pushing once a release is out.
- Build speed numbers and the reasoning behind these rules: docs/gpui/BUILD-SPEED.md.
- Startup time, how to measure it and what the first second goes to: docs/gpui/STARTUP.md.
- Build loop (see "Rust builds" below): `just check app` while iterating, `just verify <crate>` before finishing, `just verify-workspace` / `just gate` once at the end.
- Caches: builds go through sccache (.cargo/config.toml wraps rustc with scripts/rustc-wrapper, which falls back to plain rustc). Each worktree keeps its own `target/`; never share a CARGO_TARGET_DIR between worktrees, and never `cargo clean` unless a build is genuinely corrupt.

## Rust builds (fast loop, reliable finish)

Crates: `app` (the GPUI app), `core` (the backend library), `visuals` (wgpu effects), `audio` (the Rust playback engine).

- Never run `cargo build` to validate an edit; use `cargo check`. Build only when you need to run the binary (debug builds are incremental, ~5 s), and use `just profiling` (no LTO) instead of `--release` for measurements.
- Make a coherent batch of edits, then check once: `just check <crate>` (= `cargo check -p <crate>`). Read diagnostics you already have (bacon, rust-analyzer) before starting a new check.
- Don't run `cargo check --workspace`, `--all-targets`, `--all-features` or clippy after every change; don't start a second cargo command while one is running in the same worktree.
- Tests: `just test <crate> [filter]` for what you touched. Tests check behaviour the app relies on (the parser fixtures, the headless UI tests, the audio and visuals crates); no tautological tests.
- Before finishing: `just verify <crate>` for each crate you changed (fmt, check, tests, clippy). `just verify-workspace` (or `just gate`, which adds the release build) only at the very end or when a change spans several crates; CI builds the release installers.
- Shader-only changes: `just shaders` (naga), no app build.
- Bacon is for a human terminal (`bacon` at the repository root); agents don't start it.
- Worktrees: concurrent agents each use their own worktree and `target/`, all sharing sccache (worktree slots ../yt-rust-wt/slotN are listed as sccache `basedirs`). Reuse a slot for the next task (`git checkout -B <branch> main`) instead of making a new worktree, so its `target/` stays warm. Don't edit modules another concurrent task owns.

## Start here

1. [docs/gpui/PLAN.md](docs/gpui/PLAN.md): milestones, decisions and the log.
2. [docs/integration.md](docs/integration.md): verified cookie, InnerTube and stream facts and the chosen design. Read it before touching sign-in, playback or the build.
3. [docs/gpui/DESIGN.md](docs/gpui/DESIGN.md) for the look, and [docs/gpui/GPUI.md](docs/gpui/GPUI.md) for GPUI API notes, pitfalls and the UI tests.
4. If a `notes/` directory exists, it's the maintainer's private notes (gitignored). Read `notes/AGENTS.md` first and follow it as well.

## Rules

- Cookie values and the Chromium cookie key are secrets. Never log, print or commit them; derived cookie files are 0600 and short-lived. Read the browser's cookie store read-only and never restart or modify the browser.
- The app acts as the maintainer's main YouTube channel (M12). Checks never change that account or play real streams signed in: use `ENCORE_FAKE_STREAM` for playback and a fresh `XDG_CONFIG_HOME`/`XDG_CACHE_HOME` (signed out) for anything else. Keep YouTube requests few (the account has been rate limited).
- Nothing from a real account goes into the repository: no captured responses, screenshots or logs with account data. Captures and logs stay in the gitignored `artifacts/`. Public screenshots come from signed-out runs.
- Don't vendor or patch third-party crates here.
- `cargo fmt --all --check` and `just lint <crate>` (clippy `-D warnings`) must pass. Release builds are heavy (several minutes, a few GB of memory); cap them on small machines.

## UI copy

- Use YouTube Music's own labels for familiar places: Home, Explore, Library, Up next, Lyrics, Related, Quick picks, Listen again.
- Plain and short, sentence case. Say what happened and the one thing to do: "Signed out of YouTube Music" with Reconnect, not "Authentication error (401)".
- No exclamation marks or cleverness in errors. Technical detail (itag, codec, error text) goes behind a Copy button or in Settings.
- Never claim a state that wasn't checked, for example "Signed in" before an account request has succeeded.
