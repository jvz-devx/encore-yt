# Rust code-quality review

Reviewed from `98bd893` on the `quality` branch, 2026-10-08. Scope is all
six workspace crates, including examples and tests. Findings are ordered
by risk within each crate. Each item records its fix or an explicit
retention decision. Visuals implementation findings are deferred under
the orchestrator's ownership instruction.

The baseline command was:

```sh
cargo clippy --workspace --all-targets -- -W clippy::pedantic -W clippy::nursery
```

CodeGraph caller analysis, pattern searches, module reads and focused
regressions supplement the compiler audit. Raw diagnostics remain in
ignored `artifacts/quality-clippy.txt`, not in Git.

## Core

- C1, high, fixed in `58e20a4`: HTTP client construction could panic.
  `Client::new` now returns an error, and every repository caller handles
  it. Cache and snapshot lock poisoning is reported and recovered rather
  than causing another panic during cleanup. Receiver and downloaded-file
  extraction no longer relies on unchecked expects.
- C6, high, fixed in `58e20a4`: the Discord test changed process
  environment while other test threads were running. Its socket test now
  runs in a child process with the environment set before startup.
- C7, high, fixed in `70a9f20`: cookie and stream-cache writes could share
  temporary paths or reuse permissive files. The atomic writer reserves
  unique private files exclusively. Concurrent writes leave a complete
  final file with Unix mode 0600.
- C8, high, fixed in `1b61f32`: cookie imports accepted lookalike domain
  suffixes and invalid expiry fields. Schema-24 decryption accepted a
  missing or wrong host digest. Synthetic regressions cover rejection.
- C9, high, fixed in `08d62ed`: browser snapshots inherited source file
  permissions, ignored WAL failures and relied on success-path cleanup.
  Snapshots now use exclusive mode-0600 creation, retain ownership records
  for cleanup on every exit, and never remove a path they failed to create.
  Keyring lookup/close failures are no longer hidden as missing entries.
- C10, high, fixed in `8537f04`: the JS solver had an unbounded job queue
  and no execution deadline. It now queues at most eight jobs, skips
  cancelled queued work and interrupts each solve after 30 seconds.
  Signature probes are checked before conversion; failure bookkeeping is
  bounded. A synthetic infinite loop proves interruption and reuse.
- C11, high, fixed in `ee2e57d`: account writes accumulated without a cap,
  and a closed writer could leave optimistic changes pending forever.
  The queue holds 128 writes; rejected operations receive a stamped
  refusal so the frontend can roll back.
- C5, medium, fixed in `5e6eccb`: colour/session-index casts could wrap,
  and extreme playback positions could overflow Discord timestamps.
  Checked conversions and saturating timestamp arithmetic have boundary
  regressions. Other media-time float casts are retained where Rust's
  saturation is intended; format itags are selected from the fixed
  supported-itag list before their conversion.
- C3, medium, fixed in `784ecaf`, `6a94318`, `f4021e7` and `8841516`:
  resolver caches could accumulate entries, and filesystem work ran on
  async workers. Resolved streams are capped at 512 unexpired entries;
  refresh bookkeeping at 64 player versions. Player/solver/page-cache I/O
  uses async operations or the blocking pool. Settings and cookie imports
  use the blocking pool. Session saves retain one pending snapshot and
  flush it before acknowledging shutdown.
- C2, medium, fixed in `f0b9d55`: discarded player-setter results looked
  fallible but always returned `Ok(())`. These commands now return unit
  synchronously; loading stays fallible. Unreachable equalizer error
  branches were removed.
- C4, medium, fixed in `f5407e2`, `dc9eaae`, `28a0772`, `a950888`,
  `6a94318` and `1b61f32`: unrelated responsibilities shared large
  modules. Private modules now own backend protocol data, browser metadata,
  cookie codecs/cryptography, account action data and page projection,
  JSON traversal and item parsing, format selection and solver downloads.
  Public import paths remain available through explicit re-exports.
  Account overlay tests cover idempotence and unrelated-page isolation.
- C12, medium, deliberately kept: the remaining backend control/event
  mailboxes are lossless internal transports, not network-facing queues.
  Replacing their synchronous send contract with blocking sends or silent
  drops would change shutdown, playback and UI delivery. These queues
  remain unbounded by type; no hard memory bound is claimed. Network-fed
  Cast queues, solver jobs and account writes now have explicit limits.
- C13, medium, deliberately kept: persistent page/cover caches preserve
  offline results across launches. This pass bounds runtime caches but
  does not invent a disk-retention policy that deletes previously cached
  content. Disk growth remains a documented tradeoff.
- C14, low, deliberately kept: the remaining long account transaction
  ledger, native stream coordinator and playback state machine keep
  coupled state transitions together. Their large exhaustive dispatch
  functions are single-purpose. Splitting them by line count would spread
  private invariants and expose more mutable state. Auth's remaining
  import pipeline includes its local snapshot regression tests.
- C15, low, fixed in `f288a5a`: lost backend commands and missing shutdown
  acknowledgements now get diagnostics. Sends to a caller that already
  timed out, and task replies after receiver cancellation, remain normal
  teardown cases.
- C16, low, deliberately kept: `player::shared` holds an async mutex while
  opening the engine on a blocking worker. It serializes construction of
  the single process-wide audio engine. No standard mutex is held across
  this await.

## App

- P1, high, fixed in `d236dd7` and `ff3a56b`: sign-in process launch,
  pipe reads and child reaping could block the UI; preferences were
  written synchronously during interaction. The helper has one worker
  owner and a bounded cancellation/result path. Motion, prefetch, recent
  collections and update preferences use coalescing background writers.
  Explicit quit-hook flushes retain the last value even when Drop is not
  run. Tests cover cancellation, status reads and final-value persistence.
- P5, high, fixed in `497ccb4`: visited pages, scroll/animation handles
  and hover history accumulated for the whole session. The page cache is
  capped at 128 while protecting navigation history and library pages.
  Associated handles are evicted together. Hover resolution history is
  capped at 512, old hover-fetch timestamps expire, and portal delivery
  uses a bounded channel. A headless regression checks protected pages.
- P6, medium, fixed in `b4d5803`: timed-out update children were killed
  without reaping, and cleanup failures were discarded. They are now
  reaped and failures reported. A failed desktop worker no longer falls
  back to a synchronous portal connection. A synthetic process test checks
  reaping; actual installs and platform services were not run.
- P2, medium, fixed in `70a9f20`: settings readers treated unreadable or
  corrupt files exactly like missing files. Motion, prefetch, updates,
  core settings and resolver state share a reader that reports the
  operation/location without dumping contents. Missing files still default.
  Recent collections use the same policy in `497ccb4`.
- P3, medium, fixed in `e35efbf`: palette command matching/ranking and UI
  orchestration shared 879 lines. Matching now consumes borrowed page
  snapshots in its own module. Public result paths are preserved; library
  items are no longer collected just to iterate. Pure ranking and existing
  headless palette tests pass.
- P4, low, fixed in `f378c16`, `497ccb4` and `b4d5803`: unexplained
  argument-count allowances now state their UI composition boundary.
  An unused header-menu constructor and stale dead-code suppressions were
  removed. Tabular font features are initialized once instead of allocating
  each render; playlist-id comparisons borrow strings.
- P7, low, deliberately kept: initial preference reads and joins of
  startup workers occur before the first window is ready, so the first
  frame has the right settings. Final preference/backend flushes may
  block during quit to preserve durability. Neither is a per-frame path.
  Updater extraction/probing sleeps run on a blocking worker or in the
  separate helper process, not during UI rendering.
- P8, low, deliberately kept: GPUI entity updates may fail when their view
  is gone; those cancellation results remain ignored. Render elements own
  strings/images and asynchronous closures own captured state, so their
  necessary clones remain. Design-system tokens remain a consistent
  catalogue even when no current view consumes every token.
- P9, low, deliberately kept: updater inherited-descriptor cleanup already
  has a `SAFETY:` comment. It runs from `update::intercept` before logging,
  background workers or the UI start, and leaves standard descriptors
  alone. No new unsafe code was added.

## Audio

- A1, high, fixed in `4b71f5a`: large device callbacks resized scratch
  storage. Rendering now uses fixed-size stereo chunks. Regression checks
  preserve samples and timestamps while keeping scratch address/capacity.
- A2, high, fixed in `4b71f5a`: a full event ring silently lost playback
  transitions. It now retries without blocking the callback, retaining
  original end timestamps.
- A4, high, fixed in `f482f3f` and `ed78519`: network-sized allocations,
  signed seek arithmetic and unchecked media ranges could overflow or
  allocate unreasonable buffers. Compressed sources are capped at 512 MiB
  with fallible allocation. Byte ranges and deck indices are checked,
  zero codec rates rejected, and padding endpoints use checked addition.
- A9, high, fixed in `8ace7ba`: stalled range requests could wait
  indefinitely; empty responses could restart without consuming retries.
  Each request has a 30-second deadline and empty bodies use the existing
  retry budget. Local fake-server regressions cover both failures.
- A3, medium, fixed in `4b71f5a`: first-callback logging moved to the
  output-owner thread; output failures now include operation context.
- A6, medium, fixed in `f482f3f`: range merging and resampling allocated
  repeatedly; failed startup could leave an output thread waiting.
  Intervals merge in place, decoder scratch is reused, startup cleans up,
  decoder creation errors propagate and shutdown panics are reported.
- A7, medium, deliberately kept: public event and decoder-control channels
  retain their lossless non-blocking producer contract. The engine has a
  fixed deck count and a dedicated event consumer; these messages do not
  carry sample buffers. They are still unbounded by type. Adding a hard
  bound needs an explicit overload policy that does not lose seeks/end
  events or deadlock shutdown. The real-time command/sample/event rings
  are bounded.
- A5, low, fixed in `f482f3f`: the play example reports missing arguments
  instead of panicking and binds the queued track id directly.
- A10, low, fixed in `5e88611`: the new mixer regressions could write the
  global tap concurrently with its existing reader test. A test-only lock
  now serializes them; production callback behavior is unchanged.
- A8, low, deliberately kept: sample conversion to floating point is DSP
  arithmetic, not an external array index. Internal buffer/deck indexing
  follows validated layouts. Decoder/network retry sleeps run on their own
  workers, never the output callback. First-use decoder allocations and
  whole-waveform output storage remain outside that callback.

## Cast

- K1, high, fixed in `885a465`: HTTP headers were limited only after an
  unbounded line read, and a terminating oversized line bypassed the limit.
  Reading is now bounded before append; invalid Content-Length is an error.
- K2, high, fixed in `5fc4244`: pending requests survived cancellation and
  timeout, and relay/event resources were unbounded. Drop guards remove
  pending requests. Broadcasts and published sources are capped at 128,
  recent access records at 256, and active connections at 32. Request reads
  and TLS setup have deadlines; dropping the relay aborts its connections.
- K8, high, fixed in `cbcf664`: device XML could consume unbounded body
  storage or deeply recursive tree ownership. Bodies are limited to 2 MiB
  while receiving, and nesting to 64 elements. Tests cover advertised and
  actual oversize and both normal/empty nested elements.
- K3, medium, fixed in `885a465`: frame/field lengths could truncate, and
  a tenth protobuf varint byte could overflow. Encoding is fallible and
  bounded; decoding checks conversions and overflow.
- K4, medium, fixed in `5fc4244`: poisoned container locks, XML expects,
  missing SOAP responses and discarded daemon/heartbeat failures obscured
  operation failures. These now recover or return/report contextual errors.
- K7, medium, fixed in `5fc4244`: DLNA durations accepted non-finite,
  negative or extra fields. Boundary tests cover their rejection.
- K6, medium, deliberately kept: the TLS writer's async mutex serializes
  complete frames across awaits. Cast's self-signed receiver certificates
  are accepted by protocol design; handshake signatures remain verified.
- K5, low, fixed in `5fc4244` and `ad351e2`: discovery no longer sorts
  or collects just to select an address/device; socket literals use typed
  constructors, and address comparison parses once instead of formatting
  every candidate.
- K9, low, deliberately kept: Cast/DLNA wire command and status strings
  remain protocol values. Unknown receiver statuses must stay observable,
  and the spike exposes arbitrary wire commands intentionally.

## Sign-in helper

- S1, high, fixed in `64d754c`: output creation followed existing paths
  and did not repair old permissions. Exclusive creation now refuses
  existing files and symlinks. Synthetic tests check privacy and unchanged
  existing content.
- S2, medium, fixed in `64d754c`: cookie-read failure was retried forever.
  It now reports a sanitized operation error, never cookie data. Output I/O
  happens after the window/event loop closes.
- S3, low, deliberately kept: stdout's short signed-in/cancelled status is
  the parent-process protocol. Its scoped lint allowance is intentional.

## Visuals, deferred implementation

- V5, medium, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `visuals/src/pipelines.rs::DiskCache::open` feeds disk bytes to unsafe
  pipeline-cache creation. Its comment assumes unchanged `get_data`
  output. Wgpu 29's local API safety documentation requires that origin;
  adapter/header compatibility checks are not an integrity guarantee.
  Review the cache trust policy and comment together.
- V1, medium, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `spectrum.rs` has mutex expects and FFT scratch allocation; renderer,
  strip, scene and visualizer allocate per-frame uniform vectors.
- V2, medium, deferred: owned by another agent, the orchestrator hands them back after it merges.
  `app/src/visuals/frames.rs` expects image-size consistency; effects
  discard paint errors. Separate teardown-only failures from invalid frames.
- V3, low, deferred: owned by another agent, the orchestrator hands them back after it merges.
  App visuals configuration/effects mix responsibilities in large modules;
  visualizer/pipeline argument-count allowances lack reasons.
- V4, low, deferred: owned by another agent, the orchestrator hands them back after it merges.
  Borrowed frame-parameter lifetime annotations remain implicit. A scoped
  allowance records that deferral. CLI example stdout is intentional.

`6180dd0` adds the authorized panic-lint exceptions at the two visuals
roots, the lifetime exception in the visuals crate, and intended-output
annotations for its examples. The manifest inherits workspace lints.
Rendering implementations were not changed.

## Workspace decisions

- W1, medium, fixed in `f378c16` and `6180dd0`: all six manifests inherit
  workspace lints. Warnings cover unwrap/expect, dbg/todo, unintended stdout,
  undocumented unsafe blocks, unsafe operations and Rust 2018 idioms.
  Selected additional gates are checked conversions, cloned-instead-of-
  copied, flat-map-option, filter-map-next and manual-string-new. Test
  assertions and intentional binary output have reasoned local exceptions.
- W2, low, deliberately kept: pedantic/nursery are audit tools, not blanket
  gates. Must-use candidates, const suggestions, naming similarity,
  redundant-pub-crate, documentation formatting, default-trait spelling,
  exhaustive match length, closure spelling and display/DSP float precision
  warnings do not by themselves establish defects. Required ownership
  clones and explicit callback parameter lists remain.
- W3, low, deliberately kept: optional InnerTube fields, unsupported
  protocol alternatives, absent environment overrides and cache misses can
  legitimately return None/default. Required operations now report errors.
  Test unwraps remain assertions. Boolean toggle values stay booleans.
- W4, low, deliberately kept: public facades used by the app, examples and
  integration tests retain their paths. Items inside private modules remain
  bounded by module privacy; the nursery suggestion to widen their
  visibility was not followed. New implementation modules are private.
- W5, low, deliberately kept: GPUI Context lifetime elision is allowed in
  the app because the borrowed callback context determines the lifetime.
  Other Rust idiom warnings stay enabled.
- W6, low, deliberately kept: CodeGraph's index is ignored locally through
  `.gitignore`; the shared Git exclude is outside the permitted directory.

## Verification

Baseline warnings included 130 audio-library, 645 core-library,
128 Cast-library, 229 visuals-library, 1,312 app-binary and 10 sign-in-binary
diagnostics. Test duplicates are not independent findings.

Every changed crate has passed `just verify <crate>`. The final workspace
run passed 171 tests: core 61, app 46, audio 13, Cast 25, sign-in 4 and
visuals 22. The nested Discord subprocess is part of its parent test and
is not counted twice. Seven shader entry files also passed validation.
The configured pre-commit hook only handles `server/` paths, so these gates
were run explicitly.

The first workspace test build was terminated with SIGTERM after its
format and Clippy stages passed. No compiler or test failure was reported.
No Cargo/compiler process survived in this worktree; the warm-cache rerun
also includes the final tap-test isolation fix.

Final `just verify-workspace`: passed at `87bc82b`, including workspace
format checking, all-target Clippy with warnings denied, tests and shader
validation. The log is `artifacts/quality-final-verification.log`; the
intentional panic in the lock-recovery test is expected, with zero failed
tests. Only this verification record changed after the successful gate.

Temporary files and driver caches were redirected beneath ignored
`artifacts/`. No release build, sign-in,
real YouTube stream, account mutation, push, merge or tag was performed.
`Cargo.lock` is unchanged.

Tests do not prove platform behavior that they do not exercise. The offline
solver comparison returns early without captured players; GPU checks may
skip unavailable backends. No claim is made about live accounts, real audio
devices, native helper windows, Windows/macOS services or visual screenshots.
