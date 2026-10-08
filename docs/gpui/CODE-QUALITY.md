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

- C1, high, open: `innertube.rs` panics if HTTP client construction fails;
  resolver, solver and stream caches panic on poisoned locks without an
  explicit recovery policy.
- C2, high, open: `backend/deck.rs`, `backend/audition.rs` and playback paths
  discard fallible player operations. Distinguish shutdown cancellation
  from failures that affect audible playback.
- C3, medium, open: inspect cache limits and blocking persistence in
  `resolver.rs`, `jsc.rs`, `streams.rs` and backend tasks.
- C4, medium, open: `auth.rs`, `account.rs`, `parse.rs`, `streams.rs` and
  `backend/mod.rs` exceed 800 lines and mix responsibilities. Split by
  responsibility without changing their public interfaces.
- C5, medium, open: audit external numeric conversions, secret-file I/O,
  process cleanup and error context across platform paths and examples.

## App

- P1, high, open: inspect blocking work in sign-in process lifecycle,
  preference loading, desktop integration and update handoff.
- P2, medium, open: preference readers silently discard malformed files;
  distinguish missing files from unreadable or corrupt settings.
- P3, medium, open: `desktop/palette.rs` combines command definitions,
  catalogue ranking and UI orchestration in 879 lines.
- P4, low, open: unexplained `too_many_arguments` allowances in account
  sign-in and visuals-settings views; review render-path clones and public
  visibility.

## Audio

- A1, high, open: `mixer.rs::render` grows its scratch vector on the output
  callback when a device supplies a large buffer. Process fixed-size chunks.
- A2, high, open: mixer event-ring overflow silently drops playback state
  transitions. Review bounded delivery without locking the callback.
- A3, medium, open: `output.rs` logs from the first callback and lacks
  operation context on output-device and thread failures.
- A4, medium, open: review externally supplied media lengths, channel
  counts and sample-rate arithmetic in decode, HTTP and padding readers.
- A5, low, open: the `play` example panics on missing arguments and unwraps
  a queued id that can instead be bound directly.

## Cast

- K1, high, open: `http.rs` reads an arbitrarily long line before enforcing
  `MAX_HEAD`, and checks the terminator before the size limit. Invalid
  Content-Length is silently treated as zero.
- K2, high, open: `castv2/client.rs` retains timed-out or cancelled requests
  in `pending`; unsolicited events and relay access logs grow without a
  bound. Relay connections lack concurrency and request-read limits.
- K3, medium, open: `castv2/proto.rs` truncates outgoing lengths and incoming
  field sizes; a tenth varint byte can overflow `u64` silently.
- K4, medium, open: relay/pending mutex poison panics, XML stack expects,
  missing SOAP response defaults and discarded heartbeat/daemon errors
  obscure failed operations.
- K5, low, open: mDNS address selection allocates and sorts just to select
  an IPv4 address; constant socket addresses are parsed with `expect`.
- K6, kept: the Cast TLS writer uses an async mutex across writes. Whole
  frames must stay serialized; a standard mutex would block the executor.
  Self-signed receiver certificates are part of the documented Cast
  protocol. Handshake signatures remain verified.

## Sign-in helper

- S1, high, open: opening an existing output with `mode(0600)` does not
  change its old permissions and follows symlinks. Do not overwrite an
  existing path when writing session material.
- S2, medium, open: cookie-reading failures are silently retried forever;
  report failure without cookie values. Move output I/O off the event loop.
- S3, kept: stdout's `signed in`/`cancelled` messages are the helper's
  documented parent-process protocol, not debugging output.

## Visuals, review-only

- V1, medium, deliberately kept for the concurrent visuals task:
  `spectrum.rs` uses mutex `expect` calls and allocates FFT scratch in
  `bands`; renderer, strip, scene and visualizer allocate uniform vectors
  for each frame. Needs allocation and lock-policy changes in that task.
- V2, medium, deliberately kept for the concurrent visuals task:
  `app/src/visuals/frames.rs` expects image-size consistency;
  `effects.rs` discards image-paint errors. Check which are teardown-only
  and which should invalidate the frame or log once.
- V3, low, deliberately kept for the concurrent visuals task:
  `app/src/visuals/config.rs` and `effects.rs` exceed 800 lines;
  `visualizer.rs` and `visuals/src/pipelines.rs` have unexplained argument
  count allowances. Keep frame scheduling, configuration and rendering
  separate when those files are next changed.

## Workspace policy and verification

- W1, medium, open: add shared safety, panic, output and selected pedantic
  lint gates, inherit them in every crate, and scope exceptions to tests
  or documented invariants. Do not blanket-enable pedantic/nursery noise.
- W2, kept: test assertions may panic. Optional fields in loose InnerTube
  JSON and optional environment overrides may use `Option` defaults.
  Each error-discarding production path still needs its own review.
- W3, kept: CodeGraph's local index is ignored in `.gitignore`. The shared
  worktree Git exclude is outside this task's permitted write directory.

Verification is pending. No app, real stream, browser session or account
request is needed for this pass. Final completion requires crate gates and
`just verify-workspace`, not just the initial lint audit.
