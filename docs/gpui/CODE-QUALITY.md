# Rust code-quality review

Audit started 2026-10-08 at `98bd893` on `quality`. Scope includes all six
workspace crates, examples and tests. `crates/visuals` and
`crates/app/src/visuals` are review-only while another task owns them.

The initial command is `cargo clippy --workspace --all-targets -- -W
clippy::pedantic -W clippy::nursery`. Text searches cover panic sites,
discarded errors, casts, unsafe blocks, lint allowances, channels and large
modules. CodeGraph supplies symbol and caller context. Findings below are
ordered by risk within each crate. Open entries are work in progress, not
accepted debt. Fixed entries will carry the implementing commit.

## Core

- C1, high, fixed in `58e20a4`: HTTP client construction now returns a
  startup error. Resolver, solver and stream caches recover container guards
  after a worker panic and report it instead of panicking during cleanup.
- C2, medium, fixed in `f0b9d55`: the discarded player-setter results
  were always `Ok(())`. Setters now return unit synchronously; only actual
  fallible loading keeps a result. Dead equalizer error branches are gone.
- C3, medium, open: inspect cache limits and blocking persistence in
  `resolver.rs`, `jsc.rs`, `streams.rs` and backend tasks.
- C4, medium, open: `auth.rs`, `account.rs`, `parse.rs`, `streams.rs` and
  `backend/mod.rs` exceed 800 lines and mix responsibilities. Split by
  responsibility without changing their public interfaces.
- C5, medium, open: audit external numeric conversions, secret-file I/O,
  process cleanup and error context across platform paths and examples.
- C6, high, fixed in `58e20a4`: the Discord socket test mutated process
  environment while parallel tests were running. It now runs in a child
  with the synthetic socket directory set before process startup.
- C7, high, fixed in `70a9f20`: cookie and stream-cache writers shared
  predictable temporary paths and could reuse permissive files. Atomic
  writes now reserve unique private files exclusively. Concurrent writes
  leave a whole final file with mode 0600 and no temporary files.

## App

- P1, high, open: inspect blocking work in sign-in process lifecycle,
  preference loading, desktop integration and update handoff.
- P2, medium, fixed in `70a9f20`: motion, prefetch and update preferences
  share a JSON reader with core settings and resolver state. It reports
  corruption/read failure without dumping contents; missing files default.
- P3, medium, open: `desktop/palette.rs` combines command definitions,
  catalogue ranking and UI orchestration in 879 lines.
- P4, low, open: unexplained `too_many_arguments` allowances in account
  sign-in and visuals-settings views; review render-path clones and public
  visibility.

## Audio

- A1, high, fixed in `4b71f5a`: oversized output callbacks reuse fixed
  stereo scratch in chunks. Regression checks preserve samples and frame
  timestamps while keeping the scratch pointer and capacity unchanged.
- A2, high, fixed in `4b71f5a`: full event rings now delay and retry state
  transitions without blocking the callback. End events retain their
  original timestamps.
- A3, medium, fixed in `4b71f5a`: first-callback logging moved to the output
  owner thread; output failures carry operation context.
- A4, high, partly fixed in `f482f3f`: checked seek arithmetic, validated
  byte ranges, fallible allocation and a 512 MiB compressed-source limit
  replace unchecked network-sized allocation. Invalid deck indices no
  longer reach array indexing. Decoder/padding numeric review remains open.
- A5, low, fixed in `f482f3f`: missing example arguments return an error;
  the queued track id is bound directly.
- A6, medium, fixed in `f482f3f`: HTTP intervals merge in place and decoder
  resampling reuses scratch. Failed event-thread startup stops the output
  thread; decoder-thread creation errors propagate to the load caller.
- A7, medium, open: public event and decoder-control channels still use
  unbounded standard channels. Review capacity and shutdown behavior.

## Cast

- K1, high, fixed in `885a465`: `http.rs` read an arbitrarily long line before enforcing
  `MAX_HEAD`, and checks the terminator before the size limit. Invalid
  Content-Length is silently treated as zero.
- K2, high, fixed in `5fc4244`: `castv2/client.rs` retained timed-out or cancelled requests
  in `pending`. Drop guards now remove them on every exit. Unsolicited
  events are capped at 128, published sources at 128, recent access records
  at 256 and active connections at 32. Reads and TLS setup have deadlines.
- K3, medium, fixed in `885a465`: `castv2/proto.rs` truncated outgoing lengths and incoming
  field sizes; a tenth varint byte can overflow `u64` silently.
- K4, medium, fixed in `5fc4244`: relay/pending mutex poison panics, XML stack expects,
  missing SOAP response defaults and discarded heartbeat/daemon errors
  obscure failed operations.
- K5, low, fixed in `5fc4244`: mDNS address selection allocated and sorted just to select
  an IPv4 address; constant socket addresses are parsed with `expect`.
- K6, kept: the Cast TLS writer uses an async mutex across writes. Whole
  frames must stay serialized; a standard mutex would block the executor.
  Self-signed receiver certificates are part of the documented Cast
  protocol. Handshake signatures remain verified.
- K7, medium, fixed in `5fc4244`: DLNA durations accepted NaN, infinity,
  negative components and extra time fields. Reject these before they can
  become playback positions; regression cases cover each form.

## Sign-in helper

- S1, high, fixed in `64d754c`: exclusive creation refuses existing output
  paths and symlinks. Tests check mode 0600 and unchanged existing content.
- S2, medium, fixed in `64d754c`: cookie-read failures end with a sanitized
  error. Output is written only after the window and its event loop close.
- S3, kept: stdout's `signed in`/`cancelled` messages are the helper's
  documented parent-process protocol, not debugging output.

## Visuals, review-only

- V1, medium, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `spectrum.rs` uses mutex `expect` calls and allocates FFT scratch in
  `bands`; renderer, strip, scene and visualizer allocate uniform vectors
  for each frame. Needs allocation and lock-policy changes in that task.
- V2, medium, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `app/src/visuals/frames.rs` expects image-size consistency;
  `effects.rs` discards image-paint errors. Check which are teardown-only
  and which should invalidate the frame or log once.
- V3, low, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `app/src/visuals/config.rs` and `effects.rs` exceed 800 lines;
  `visualizer.rs` and `visuals/src/pipelines.rs` have unexplained argument
  count allowances. Keep frame scheduling, configuration and rendering
  separate when those files are next changed.
- V4, low, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `scene.rs` and `visualizer.rs` omit borrowed frame-parameter lifetimes.
  The visuals crate alone allows this lint pending its task. The two CLI
  examples explicitly allow their intended stdout reports.

The orchestrator authorized scoped lint annotations for these protected
paths. Both visuals roots allow only the deferred panic lints, and the
visuals crate inherits the workspace policy through its manifest. No
rendering implementation was changed.

## Workspace policy and verification

- W1, medium, fixed in `f378c16` and `6180dd0`: every crate inherits shared
  safety, panic, stdout, Rust idiom and five focused conversion/iterator
  lints. Tests and CLI output have explicit reasons for their exceptions.
  GPUI context lifetime elision is deliberately retained because the
  borrowed callback context determines that lifetime. Protected visuals
  exceptions are tracked under V1-V4; other production panic sites stay gated.
- W2, kept: test assertions may panic. Optional fields in loose InnerTube
  JSON and optional environment overrides may use `Option` defaults.
  Each error-discarding production path still needs its own review.
- W3, kept: CodeGraph's local index is ignored in `.gitignore`. The shared
  worktree Git exclude is outside this task's permitted write directory.

Baseline pedantic/nursery diagnostics are retained locally in
`artifacts/quality-clippy.txt`. They include 130 audio library warnings,
645 core library warnings, 128 cast library warnings, 229 visuals library
warnings, 1,312 app binary warnings and 10 sign-in helper warnings. Counts
overlap with test targets and are not a count of independent defects.

`TMPDIR="$PWD/artifacts" just verify cast` passes 23 tests, formatting,
check and Clippy with warnings denied at `5fc4244`. The configured
pre-commit hook only checks `server/` paths and does not enforce this
workspace's gates, so each gate is run explicitly.

Crate gates also pass for sign-in at `64d754c`, four tests; audio at
`f482f3f`, 11 tests; and core at `58e20a4`, 31 unit and 16 integration tests.
The offline resolver comparison returns early when no player captures are
available, so that result does not prove a live YouTube challenge works.
After private persistence changes, core passes 49 tests and app passes 38
tests through their full gates. The visuals inheritance commit removes the
previous protected-file lint failure.

## Module boundaries under review

The Rust architecture review keeps public paths stable and splits only
independent responsibilities. `backend/protocol` will own command/event
data, used by the worker and UI through explicit backend re-exports.
`auth/browser` will own platform browser metadata and installation lookup,
used by cookie discovery and desktop browser reporting. Neither child
depends on its parent's orchestration. Existing parser, auth, browser and
headless UI tests are the behavior checks for those moves.

Final verification is pending. No app, real stream, browser session or account
request is needed for this pass. Final completion requires crate gates and
`just verify-workspace`, not just the initial lint audit.
