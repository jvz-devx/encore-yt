# Audio without mpv (M11 decision record)

2026-10-07. Question: can a pure Rust pipeline replace the mpv subprocess, so the app ships as one binary plus yt-dlp and deno?

Answer: yes. The spike crate `crates/audio` (`encore-audio`) plays WebM/Opus and fragmented MP4/AAC over HTTP with range requests, seeks, joins tracks without a gap, applies the 10-band EQ, and does it with about half of mpv's CPU and a fifth of its memory. It adds about 3.8 MB to the app. The work to swap it into the backend is real but bounded, and most of the risk sits in YouTube's container details, which this spike could not check against real streams (no YouTube streams by rule). Recommendation at the end.

## What the spike is

```text
HttpSource (Range requests, read-ahead) -> symphonia demux (WebM, MP4)
  -> decode (libopus, symphonia AAC) -> stereo -> rubato -> track ring
  -> mixer (3 decks, gapless next, volume, loudness gain) -> EQ -> cpal
```

| File | Lines | What it does |
|---|---|---|
| `crates/audio/src/http.rs` | 430 | One buffer the size of the file, filled by a fetch thread with 2 MiB `Range` requests up to 32 MiB ahead of the reader. A read the running request won't reach within ~0.25 s (measured throughput) drops that request and starts one at the read position. Downloaded bytes stay, so seeking back is free. Network errors retry the chunk 3 times; an HTTP status (403 on an expired URL) ends the track with an error. |
| `crates/audio/src/decode.rs` | 440 | One thread per track: probe, decode, mix down to stereo, resample, push into a 2 s lock-free ring (`rtrb`). Seeks decode from 80 ms before the target and drop up to it (Opus and AAC both need a pre-roll). A seek gives the mixer a fresh ring, so no stale audio plays. |
| `crates/audio/src/padding.rs` | 105 | Opus end padding from WebM's `DiscardPadding` (symphonia ignores it, see below). |
| `crates/audio/src/resample.rs` | 95 | rubato's FFT resampler with its delay cut and its tail flushed, so N input frames give exactly `round(N * out / in)` output frames. AAC is 44.1 kHz, the device 48 kHz. |
| `crates/audio/src/mixer.rs` | 320 | The output callback. Three decks (main, smooth mix, audition), each with a current and a queued next track. The next one starts in the same buffer the current one ends in. Per-deck volume and pause glide over 5 ms, each track has its own loudness gain, a 100 ms prebuffer applies at start and after seeks. No locks, no waiting on the network. |
| `crates/audio/src/eq.rs` | 140 | RBJ peaking biquads, one octave wide, at the backend's ten centres (31 Hz to 16 kHz), with the same `-max(gain)` headroom preamp. This is what FFmpeg's `equalizer=t=o:w=1` computes, so presets sound the same as today. |
| `crates/audio/src/output.rs` | 120 | cpal on its own thread (so the engine is `Send`). On Linux it speaks the PulseAudio protocol to pipewire-pulse with cpal's pure Rust client, else ALSA; WASAPI on Windows, CoreAudio on macOS. 2048-frame periods. |
| `crates/core/src/lib.rs` | 355 | `Engine`: `load`, `queue`, `clear_next`, `seek`, `pause`, `set_volume`, `stop`, `set_equalizer`, `stats`, and an event channel (`Started`, `Ended` with the played length, `Seeked`, `Error`). |

Run it:

```sh
cargo run -p encore-audio --example serve -- artifacts/audio 8765 1048576   # dir, port, bytes/s (0 = no limit)
cargo run -p encore-audio --example play -- http://127.0.0.1:8765/a.webm http://127.0.0.1:8765/b.webm
```

`play` loads the first URL, queues the second for a gapless join, seeks to 30 s after 5 s, turns on Bass boost after 8 s, and prints a timestamp per step (env `SEEK_AT`, `SEEK_TO`, `EQ_AT`, `TAIL`, `GAIN_DB`, `STOP_AT`). At the end it prints its CPU time and peak RSS. `serve` logs every request with its `Range` header and when a client drops a response.

Test files were made with ffmpeg from `artifacts/test-audio.opus` and a 440 Hz tone. Opus 160k in DASH WebM (`-dash 1 -cues_to_front 1`, like itag 251/774) and AAC-LC 128k at 44.1 kHz in fragmented MP4 with a global `sidx` (`-movflags dash+global_sidx`, like itag 140). The tone tracks are one 90 s tone cut at 45.000 s into A and B, so any gap at the join shows as a dip.

## Measurements

Fedora 43, i5 with 6 cores, load average 14 to 23 from other agents throughout. `--profile profiling` build unless noted. Local server, so network round trips are not in these numbers. Expect googlevideo to add 100 to 300 ms to first audio and to every seek that needs a new request.

| | encore-audio | mpv 0.40 (same file, same EQ graph) |
|---|---|---|
| CPU, 60 s of the 3 min song, EQ on | 1.5 % of one core (Opus), 1.4 % (AAC + resampling) | 2.6 %, 2.5 % |
| Peak RSS | 17.8 MB, 16.2 MB | 90.6 MB, 92.5 MB |
| First audio after `load` | 15 to 56 ms | not measured |
| Seek, target already downloaded | 10 to 61 ms | |
| Seek at 1 MiB/s, WebM (new range request) | 195 ms | |
| Seek at 1 MiB/s, fragmented MP4 | 298 ms | |
| Seek at 512 KiB/s, WebM / fragmented MP4 | 300 ms / 2.6 s | |
| Release binary of `play`, stripped | 7.27 MB | |
| Same with only reqwest + rustls (already in the app) | 3.49 MB | |
| Runtime libraries | `libasound.so.2`, libc, libm, libgcc_s | mpv + FFmpeg + libplacebo and friends |

So the audio stack adds about 3.8 MB to a 59 MB app binary. libopus is built from source by `opusic-sys` (CMake and a C compiler, which the build already needs) and linked statically.

Range requests on seek, from the server log at 64 KiB/s (`artifacts/audio/logs/serve-webm.log`, gitignored):

```text
[   0.432] #0 GET tone-a.webm Range: 0-2097151 -> 0-1010349/1010350
[   0.465] #1 GET tone-b.webm Range: 0-2097151 -> 0-1012133/1012134
[   5.686] #2 GET tone-a.webm Range: 561480-1010349 -> 561480-1010349/1010350
[   6.183] #0 client dropped the response after 376832 bytes (Broken pipe (os error 32))
```

Request #1 is the queued next track downloading while the first plays. That is the prefetch.

### Gapless

The mixer inserts 0 frames at a join: track A's `Ended` and track B's `Started` carry the same output frame in every run. Whether the join is clean then depends on the container trimming, which I checked two ways. The played length of A in output frames, and a `pw-record` capture of the player's own PipeWire stream, scanned for 1 ms windows below half the tone's level.

| | A's played length (45.000 s = 2,160,000 frames) | Capture around the join | FFmpeg's decode of the same file |
|---|---|---|---|
| WebM/Opus, before the padding fix | 2,160,648 (+13.5 ms) | 13 ms dip | 2,160,000 |
| WebM/Opus, with `padding.rs` | 2,160,000 | no dip anywhere in 47 s | 2,160,000 |
| Fragmented MP4/AAC | 2,161,128 (+23.5 ms) | 23 ms dip | 1,985,536 at 44.1 kHz, also +23.5 ms |

The WebM join is sample exact. The AAC join keeps the encoder's 1024 priming frames plus a few padding frames. FFmpeg (so mpv) keeps them too, because a fragmented MP4 without an edit list doesn't say how much to cut. For a progressive MP4 with an edit list, FFmpeg cuts the priming and symphonia doesn't (symphonia 0.6.1 doesn't read `elst`).

Early recordings over PipeWire's ALSA plugin with cpal's default buffer (two ~21 ms periods) had 4 to 10 ms dropouts every few seconds under this load. With 2048-frame periods and the PulseAudio protocol both 47 s captures were clean. A capture of mpv under the same load had one 21 ms dropout.

## What symphonia gets wrong today (0.6.1)

- **WebM `DiscardPadding` ignored.** Every Opus track ends with up to 20 ms of padding. Fixed here by reading the last BlockGroup from the downloaded tail and holding back one decoded packet to trim. Worth an upstream patch; the `Packet::trim_end` field already exists.
- **MP4 edit lists ignored.** Matters only for progressive MP4; YouTube's 140/141 are DASH files. Needs a check on real streams.
- **Fragmented MP4 seeks scan fragment headers one by one** (`try_read_more_segments`) instead of jumping with `sidx`. Over HTTP that means waiting for the download to reach the target (2.6 s at 512 KiB/s). Once the file is downloaded, seeks take ~10 ms, and a 3 MB AAC track downloads in well under a second at typical googlevideo speeds. Opus (774, 251) is the format we prefer anyway; an upstream `sidx` seek would fix AAC.
- **No HE-AAC.** The AAC decoder refuses explicit SBR ("aac too complex") and would play only the core of implicit SBR. Itag 139 (HE-AAC 48 kbps) is the last fallback in `-f 774/141/251/140/250/249/139`; drop it or keep mpv for it.

## Replacing mpv in the backend, item by item

| mpv today | With encore-audio | Work |
|---|---|---|
| `Mpv` process per deck, JSON IPC, property observers (`time-pos`, `pause`, `playlist-pos`, `idle-active`, `paused-for-cache`, `seeking`), events tagged with a process serial | One `Engine`; decks are indices. `Event::Started/Ended/Seeked/Error` carry the deck and a track id that plays the role of mpv's playlist entry id and process serial. Position comes from `stats()` on a timer or a new position event. | Replace `src/mpv.rs` (270 lines) with an adapter; rewrite the event routing in `playback.rs` and `deck.rs` against track ids. The biggest single piece. |
| Gapless: `loadfile append`, detect the move to playlist position 1, remove position 0 | `queue()` and `clear_next()`; the engine promotes next to current itself. The "two entries at most" bookkeeping and the idle safety net go away. | Small, and simpler than today. |
| Smooth mixes: a second deck cued paused at volume 0, then crossfaded by `volume` | `load(deck 1)`, `pause(1, true)`, then `set_volume` on both decks from the same 20 ms clock; amplitudes are linear now, so drop the cube root mpv's cubic `volume` needed. | Port the deck role logic; keep the equal-power curve. |
| Audition: a third deck, 250 ms fades, main deck ducked to 0.2 | Same with deck 2 and `set_volume`; `Load.start` is the start time. | Small. |
| Loudness: per-file `volume-gain` that reverts at the file's end | `Load.gain_db`, applied per track from its first sample, so it is exact across a gapless join too. | Trivial. |
| EQ: a lavfi graph in `af`, `af-command` for live edits, rewritten 600 ms after edits stop, sent to every deck | `set_equalizer(Some(gains))` on the summed output: one call, no debounce, no graph rebuild. | Trivial; delete the lavfi string code. |
| Cache: `--cache=yes --demuxer-max-bytes=64MiB` | The whole file in memory (3 to 10 MB per song), 32 MiB read-ahead, the next track downloading as soon as it is queued. Could also feed the waveform (below). | A memory cap for hour-long mixes; otherwise done. |
| Stream errors: mpv `end-file` with an error, retry once through yt-dlp, then skip | `Event::Error` with a message; HTTP status errors end at once, network errors retry 3 times inside the reader. | Map errors to the existing retry; add "swap URL, keep the bytes" for expiry (below). |
| User agent per file | `Load.headers`. | Trivial. |
| Seeking, `start` per file | `seek()`, `Load.start`; ~10 to 60 ms from cache, one range request otherwise. | Done. |
| Repeat one: `loop-file=inf` | Seek to 0 at `Ended`, or queue the same URL as next (the bytes are cached). | Small. |
| Sleep timer fade, pause at song end | `set_volume` and `pause`; the engine reports `Ended` so "end of song" needs no change. | Small. |
| Output: mpv picks the system's audio API | cpal: PulseAudio protocol or ALSA on Linux, WASAPI, CoreAudio. Only Linux was run here. | Test on Windows and macOS; handle a device going away (cpal reports a stream error; reopen on the new default). |
| Spectrum: `pw-record` plus `pw-link` to mpv's PipeWire nodes, Linux only | A tap on the mixer's output ring, in-process, on every platform. | Medium; removes `visuals/src/pipewire.rs`. |
| Waveform: asks mpv for the URL over IPC, then runs `ffmpeg` to decode it again (a second download) | Decode the cached bytes with symphonia at 8 kHz. No second download, no ffmpeg. | Medium; removes `visuals/src/mpv.rs` and the ffmpeg dependency. |
| A plain kill leaves mpv playing (PLAN log, M0) | Audio dies with the process. | None. |

## Size of the work

The spike is 2,000 lines of library. To ship it I'd plan five tasks, each about the size of one PLAN milestone:

1. Harden the engine. A memory cap, swapping a URL mid-track while keeping the downloaded bytes, device loss and reconnect, no allocation or free in the callback (old rings and the EQ settings go back to a control thread), a position event, an upstream PR for `DiscardPadding`.
2. A backend adapter in the root crate behind a cargo feature, keeping `Event::Playback` as it is: replace `Mpv` in `playback.rs`, `sound.rs` and the gapless bookkeeping.
3. Port decks, smooth mixes and audition (`deck.rs`, `audition.rs`) to engine decks.
4. Visuals: the in-process spectrum tap and the waveform from cached bytes.
5. Packaging and checks. Drop mpv from `release.yml`, the README and the `.deb`/`.rpm` dependencies; run the E2E suite and a small set of real streams (774, 251, 141, 140, 250, 249) for container quirks, gapless and seeking.

About 2,600 lines of backend code touch mpv today (`mpv.rs`, `playback.rs`, `deck.rs`, `sound.rs`, `audition.rs`, `equalizer.rs`); maybe half of it is mpv-specific and gets simpler.

## Installer size

- Windows drops the shinchiro mpv build: a 34 MB `.7z` download today (2026-10-07), unpacked into `bin\`.
- macOS no longer needs `brew install mpv`, which pulls FFmpeg and a long dependency tree.
- Linux: the AppImage no longer needs a system mpv, and the `.deb`/`.rpm` drop the mpv dependency (on Fedora: mpv 4.6 MB plus FFmpeg's libraries, ~18 MB).
- The app grows by ~3.8 MB.
- yt-dlp (18 MB) and deno (43 MB zipped) stay, so it is not a single binary yet. Stream resolution still needs them.

## Risks

- **Real YouTube files are untested here.** Cue density in 251/774 WebM (seek cost depends on it), whether 140/141 carry an edit list, fragment length in the DASH MP4s. The first real-stream run must check seeking, played lengths and joins on each itag.
- **SABR.** If YouTube moves audio to SABR/UMP-only delivery, mpv and this engine break the same way. With our own reader we can at least add a UMP reader in Rust. With mpv we can't.
- **URL expiry mid-track.** googlevideo URLs expire after hours, so this mostly hits a paused song resumed later. Today mpv fails and the backend retries through yt-dlp. With the engine we can resolve again and continue at the same byte offset with the downloaded bytes kept. That's better than now but has to be built (task 1).
- **googlevideo throttling.** The reader asks for 2 MiB ranges, as yt-dlp does for this reason. Not tested against googlevideo.
- **Codecs.** Opus and AAC-LC only; no HE-AAC (itag 139). That covers every format the resolver prefers.
- **Platforms.** Only Linux was run. WASAPI and CoreAudio through cpal are common setups, but device switching (Bluetooth, unplugging) needs its own tests on each.
- **Real-time behaviour.** The callback thread has normal priority. It held up at load 20 with an 85 ms buffer over PulseAudio. ALSA with small buffers did not. cpal's `realtime` feature (rtkit) is an option if needed.
- **One process.** A crash in decoding now takes the app down instead of one mpv. symphonia is safe Rust; libopus is C but mature, and mpv runs the same C code.

## Recommendation

Replace mpv with `encore-audio`, in the five tasks above, behind a cargo feature with mpv as the fallback until a real-stream check of each itag passes (gapless, seeking, played length). Then drop mpv from the installers. It costs ~3.8 MB, uses half the CPU and a fifth of the memory, makes gapless joins and per-track gain exact, gives the visuals an in-process tap and a waveform without ffmpeg, and removes the one runtime dependency we can't bundle on macOS and Linux. Do the engine hardening first. The fragmented-MP4 seek and `DiscardPadding` gaps are upstream symphonia work worth sending.

## M19: the Rust engine in the backend (2026-10-07)

Superseded by M23 below: mpv, the engine choice and the fallback are gone. Kept as the record of how the engine got there.

The backend now plays through `crates/core/src/player/` (`Player`, one deck). mpv is one engine, unchanged in behaviour; `encore-audio` is the other, behind the root crate's `rust-audio` feature, which the GPUI app turns on. There it is the default; `YTFAST_PLAYER=rust|mpv` and Settings' Audio player (Built-in or mpv, from the next song) choose.

`crates/core/src/player/rust.rs` keeps mpv's playlist model on top of the engine, so `playback.rs`, `deck.rs`, `audition.rs` and `sound.rs` work unchanged on either engine:

| Backend asks | mpv | Rust engine |
|---|---|---|
| `load` replace / append | `loadfile` with per-file options | `Engine::load` / `queue` on the deck; entry ids kept here |
| gapless change | playlist position 1, then remove 0 | the engine's join; the same `EndFile eof`, `StartFile`, `PlaylistPos(1)` events |
| `skip` (Next with the next song queued) | `playlist-next force` | `Engine::skip`: the queued track, already downloading, plays at once |
| repeat one | `loop-file=inf` | the next track stays out of the engine; the song loads again at its end |
| start, gain, user agent | per-file options | `Load.start` / `start_share` (Audition's "a third in"), `gain_db`, `headers` |
| live gain | `volume-gain` | `Engine::set_gain` |
| volume | `volume` (cubic) | the same cubic scale as amplitude |
| equalizer | `af`, `af-command` per deck | the engine's one output EQ, set when the gains change |
| position, duration, buffering | observed properties | a 100 ms poll of the engine's counters; a playing track that hasn't moved for 3 polls is buffering |
| errors | `end-file error` | `Event::Error` as `EndFile error`; a queued track's error surfaces when it would start |

The engine gained what this needed: local files and Ogg (so `ENCORE_FAKE_STREAM` works), `skip`, `set_gain`, the track's length (WebM gives it for the segment only), a start as a share of the length, six decks, and `take_events`.

Which engine plays a song (`src/backend/engines.rs`): the chosen one, except that with Rust chosen, formats it has no decoder for (HE-AAC 139 and 599) and songs it failed on in this run play on mpv, logged once per song. A song on the other engine than the main deck starts as a new song (no gapless change). Smooth mixes and Audition start their deck on the song's engine. Installers still ship mpv for this.

Visuals: the waveform decodes the resolved stream's URL (`Backend::stream_url`) instead of asking mpv; the spectrum tap also links the engine's PipeWire stream, which pipewire-pulse names `cpal-pulseaudio-<pid>`.

### Checked

In the GPUI app with `ENCORE_FAKE_STREAM` (the test Ogg file), signed in, controlled through MPRIS (`playerctl`) and the window: play, pause, seek, Next (skip to the queued track), Previous, Space, a gapless join with each song's own loudness gain (−4.23 dB, then −2.90 dB), the Bass boost preset at start and a live change to Vocal from Settings, a smooth mix (equal-power volumes on both decks over 6 s, then the old deck cued with the next song), Audition (main deck ducked to 58.5, audition faded in over 250 ms from a third in, both back on release), the sleep timer at the end of the song (fade over the last 8 s, pause, next song parked), switching to mpv in Settings (the main deck moved to mpv at the next song), Now Playing's spectrum and waveform, and the fallback (an MP3 file the engine has no reader for played on mpv, logged once per song).

Real streams, signed out, one public music video resolved with yt-dlp (`crates/core/examples/stream_check.rs`, no playback tracking):

| itag | first audio | seek right after start (range request) | seek, downloaded | played vs container | join |
|---|---|---|---|---|---|
| 251 Opus | 40 ms | 87 ms | 29 ms | −16.4 ms (end padding trimmed) | 0 frames |
| 250 Opus | 48 ms | 86 ms | 27 ms | −16.4 ms | 0 frames |
| 249 Opus | 132 ms | 86 ms | 29 ms | −16.4 ms | 0 frames |
| 140 AAC | 49 ms | 344 ms (fragment scan) | 32 ms | ±0 | 0 frames |

Premium 774 and 141 need a signed-in resolve: `cargo run --example stream_check --no-default-features -- --signed-in --formats 774,141 VIDEO_ID`.

CPU and memory in the app (profiling build, Home showing, a song playing, load average ~1): mpv 6.0 % (app) + 1.6 % (mpv) of a core, 234 + 86 MB PSS; Rust engine 7.3 % and 209 MB PSS, the engine included.

### Still open

- The engine's output stream stays open while the app runs (silence when nothing plays); mpv's closes when idle.
- A seek ahead of the download can fetch some bytes twice (itag 251 downloaded 4.25 MB of a 3.43 MB file after an early seek).
- From the engine hardening list above: no allocation or free in the callback, device loss, a memory cap for long mixes, swapping an expired URL in place. Windows and macOS output untested.

## M23: the only player (2026-10-07)

mpv is gone from the code (`src/mpv.rs`, `src/backend/engines.rs`, the `rust-audio` feature, `YTFAST_PLAYER`, Settings' Audio player) and from every installer. `player::Player` is one deck of the engine. The resolver never picks HE-AAC (139, 599), so every format it hands over has a decoder. A stream or decode error ends the file with an error: the song is resolved once more without the account, then skipped with "Couldn't play “…”, skipped it" (the decoder's words behind Copy details); three failures in a row stop playback with a plain message instead of running through the queue.

M22 came with it: the spectrum reads the engine's tap (`crates/audio/src/tap.rs`: the output callback copies the mix as mono into a ring of atomics while a reader is open; no lock, no allocation, nothing it waits on) and the waveform decodes the cached stream URL with the engine's reader and decoders (`crates/audio/src/whole.rs`, `decode_mono`), so neither PipeWire's tools nor ffmpeg are needed, on any OS.

### Checked

- In the GPUI app (debug build, fresh signed-out config and cache, `ENCORE_FAKE_STREAM`): play, Next, seeks (landed at 9.96 s for 10.06 s, 15.98 s for 16.08 s), the spectrum at 60 hops per second from the tap (601 hops in 10 s) and the waveform decoded in 1.27 s for 180 s of audio, both visible in Now Playing; the Bass boost equalizer from settings at start; Audition (a second deck from a third in, let go after 3 s); a smooth mix on a radio (blend over 5.6 s after a seek to 174 s, the cued deck taking over).
- Real streams, signed out, one public song through `crates/core/examples/stream_check.rs` (resolved by `crates/core/src/streams.rs` as VISIONOS in 0.2 s): itag 251 first audio 42 ms, seeks 28–86 ms, played 213.045 of 213.061 s, join 0 frames; itag 140 first audio 44 ms, seeks 30–259 ms, played to the frame, join 0 frames.
- Premium 774 and 141 need the signed-in check above (not run here).

### Size

The installers lose mpv and its libraries (Windows: a 34 MB `.7z`; the AppImage's mpv, FFmpeg and libplacebo build; macOS: mpv.app's libraries), yt-dlp (18–40 MB per platform) and deno (~42 MB zipped, ~138 MB unpacked): on the order of 100 MB compressed per installer, leaving the app's own executable.
