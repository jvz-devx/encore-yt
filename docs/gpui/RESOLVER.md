# M14: resolving streams without yt-dlp

Research, the implementation behind `YTFAST_RESOLVER=rust`, what was measured on 2026-10-07, and the recommendation. YouTube changes often: every fact here is dated.

## State of the art (October 2026)

### Clients that still get plain audio URLs

| Client | Signed out | Signed in | Challenges | PO token |
|---|---|---|---|---|
| `VISIONOS` 1.02 | yes (yt-dlp's signed-out default since 2026-07-09) | cookies not supported | none | none |
| `TVHTML5` 5.x (`tv_downgraded`) | yt-dlp: yes | yes | sig + n | none |
| `WEB_EMBEDDED_PLAYER` | embeddable videos only | yes | sig + n | none; SABR-only in some sessions since 2026-09 |
| `WEB_CREATOR` | no | required | sig + n | none for Premium, else GVS |
| `WEB_REMIX`, `WEB`, `MWEB` | `WEB` is SABR-only | | sig + n | GVS PO token unless Premium |
| `IOS`, `ANDROID`, `ANDROID_VR` | | | none | GVS or player PO token; `android_vr` dropped 2026-08-18 (403s) |

yt-dlp 2026.08.19's defaults: signed out `visionos, web`; signed in `web_embedded, tv_downgraded, web`; Premium `web_creator, tv_downgraded, web` (`yt_dlp/extractor/youtube/_video.py`, `_base.py`).

- yt-dlp added `visionos` in PR #17184 (2026-07-09). NewPipeExtractor had already moved to it on 2026-06-07 to get around SABR-only answers (PR #1508). <https://github.com/yt-dlp/yt-dlp/pull/17184>, <https://github.com/TeamNewPipe/NewPipeExtractor/pull/1508>
- `tv_downgraded` was added in 2025-11 because an older TV version "seems to prevent only SABR formats". <https://github.com/yt-dlp/yt-dlp/pull/14887>
- `android_vr` left the defaults on 2026-08-18. <https://github.com/yt-dlp/yt-dlp/pull/17461>
- Since 2026-09-10, some sessions get SABR-only `mweb` and `web_embedded` answers (issue still open). <https://github.com/yt-dlp/yt-dlp/issues/17666>

### Signature and n challenges

- Formats from JS-player clients carry either a `signatureCipher` (`url`, `s`, `sp`) or a URL whose `n` parameter must be transformed, or googlevideo throttles or refuses the request. Both functions are in the player script (`/s/player/<id>/player_ias.vflset/en_US/base.js`, about 3 MB). The request must send the player's `signatureTimestamp` so the URLs are encrypted for that version.
- yt-dlp dropped its regex-based extraction because the functions are now "spread out all over the player". Since 2025.11.12 it requires a JS runtime. <https://github.com/yt-dlp/yt-dlp/issues/14404>, <https://github.com/yt-dlp/yt-dlp/issues/15012>
- **EJS** (<https://github.com/yt-dlp/ejs>, Unlicense, bundles meriyah and astring) works like this:
  1. Parse the whole player with meriyah and keep its top-level statements.
  2. Find the n and signature candidates by AST shape, wrapped so that every candidate must agree.
  3. Regenerate the code with astring, with `window`, `document`, `navigator` and `location` stubbed.
  4. Run it in deno, node, bun or QuickJS. The regenerated script is the "preprocessed player".

  EJS had 11 releases from 2025-10 to 2026-03 and none since: its matcher has survived 6.5 months of player changes. <https://github.com/yt-dlp/ejs/releases>
- A signature transform only reorders and drops characters, so yt-dlp solves it once per signature length with the probe `chr(0)…chr(len-1)` and caches the index list (`sigfuncs`). n answers are per value.
- **YouTube.js** (LuanRT) has its own meriyah-based extractor that builds a minimal script, and the caller supplies the `eval`. <https://github.com/LuanRT/YouTube.js/blob/main/src/core/Player.ts>
- **rustypipe** uses regex extraction with rquickjs 0.9. Its last release was 2025-04 and issue #75 (2026-03) reports "could not extract sig fn name": unmaintained. <https://codeberg.org/ThetaDev/rustypipe/issues/75>
- **ahaoboy/ytdlp-ejs** is an MIT Rust port of EJS (SWC with rquickjs or boa). It is small (3 stars) but worth watching. <https://github.com/ahaoboy/ytdlp-ejs>

### PO tokens

- GVS PO tokens come from BotGuard (web), DroidGuard or iOSGuard attestation and are bound to the video id. <https://github.com/yt-dlp/yt-dlp/wiki/PO-Token-Guide>
- BgUtils 4.0 (2026-07-18) "does not bypass BotGuard": it needs a runtime that passes BotGuard's checks, and its example uses JSDOM. It documents cold-start tokens, which cover the first ~1–2 MB of media. <https://github.com/LuanRT/BgUtils>
- bgutil-ytdlp-pot-provider runs on Node or Deno with `canvas`. rustypipe-botguard embeds deno_core (V8) plus JSDOM and is kept out of the main crate because of its size. <https://github.com/Brainicism/bgutil-ytdlp-pot-provider>, <https://codeberg.org/ThetaDev/rustypipe-botguard>
- Nobody runs BotGuard in QuickJS or boa without a DOM. A PO-token path means shipping V8, a DOM and a canvas, which is the size this milestone wants to drop.

### SABR

- SABR (server ABR over the UMP protocol) streams media from `serverAbrStreamingUrl` and the server picks the chunks. `WEB` has been SABR-only since 2025-02. <https://github.com/yt-dlp/yt-dlp/issues/12482>
- yt-dlp's SABR downloader (PR #13515) is still under review on 2026-10-04, and with `web` it needs a PO token. <https://github.com/yt-dlp/yt-dlp/pull/13515>
- The reference implementation is LuanRT/googlevideo (TypeScript). In Rust there is only FineFindus/sabr (GPLv3, not on crates.io). <https://github.com/LuanRT/googlevideo>, <https://github.com/FineFindus/sabr>

### Engines

- **rquickjs 0.14** (2026-09-17) bundles quickjs-ng 0.16.2 and compiles it with `cc`. It ships pregenerated bindings for Linux gnu/musl, macOS x86_64/aarch64 and Windows msvc/gnu (MSVC is marked experimental), so it needs no bindgen or libclang. <https://github.com/DelSkayn/rquickjs>
- **boa 0.22** is pure Rust but about 2× slower than QuickJS on the EJS cases in ytdlp-ejs's benchmark. <https://github.com/ahaoboy/ytdlp-ejs/blob/main/blog.md>
- quickjs-ng before 0.12 took minutes on EJS; rope strings in 0.12 fixed that. <https://github.com/yt-dlp/yt-dlp/wiki/EJS>, <https://github.com/quickjs-ng/quickjs/issues/1002>

## What was built

All of it is off by default. `YTFAST_RESOLVER=rust` turns it on, and yt-dlp stays the fallback.

- `src/jsc.rs`: the EJS solver in rquickjs, with yt-dlp-ejs 0.8.0 vendored in `src/jsc/` (the SHA3-512 hashes match the ones yt-dlp 2026.08.19 pins; licence in `src/jsc/EJS-LICENSE`, meriyah's and astring's in the bundle headers).
  - It preprocesses a player once per version and saves `<id>.ejs.js` beside it.
  - One engine thread keeps the last player's `n` and `sig` functions loaded.
  - Signatures are solved once per length as an index list and applied in Rust.
- `src/streams.rs`: the resolver.
  - It sends the InnerTube `player` request to www.youtube.com: as `VISIONOS` when signed out, as `WEB_CREATOR` with the session's cookies and SAPISIDHASH when signed in.
  - It picks the best audio format (774/141/251/140/250/249/139, skipping DRC copies), deciphers `s` into `sp`, transforms `n`, and returns the same `Stream` with the URL's `expire`.
  - It reads the player version from `https://www.youtube.com/iframe_api` at most every 6 hours and downloads the player once per version into `~/.cache/ytfast/player/`, keeping only the current one.
  - A version not yet preprocessed is prepared in the background while that song falls back to yt-dlp.
- `src/innertube.rs` keeps the `responseContext.visitorData` from YouTube Music's responses and sends it with stream requests: signed out, VISIONOS fails the bot check without it. `player_as` sends a player request as another client.
- `src/resolver.rs` tries the Rust resolver first. On any error yt-dlp runs instead, and a song whose Rust-resolved stream failed to play (the playback retry calls `forget`) goes to yt-dlp next time. Caching, slots and priorities are unchanged.
- Tests: `tests/resolver_offline.rs` and `scripts/ejs-expected.sh`. The live check is `examples/resolve_rust.rs`.

## Measurements (2026-10-07, i5 6-core shared with other builds, debug build with deps at opt-level 2)

### Offline (`cargo test --no-default-features --test resolver_offline`)

Player `1b3be681` (signature timestamp 20728), saved under `artifacts/resolver/`.

- QuickJS's answers equal yt-dlp's EJS in deno for 6 n challenges and the signature index lists for lengths 90–120.
  - Two of the n challenges are real: the `web` SABR URL's and the `WEB_CREATOR` cipher URL's.
  - The real signature had length 108.
- Timings:

  | Step | QuickJS | deno |
  |---|---|---|
  | Preprocess, once per player version | 12–41 s, depending on machine load | about 1.2 s, including the solves |
  | Load the saved preprocessed player, once per process | 0.5–1.2 s | |
  | One warm n solve | 3–6 ms | |

### Live

10 requests to YouTube's player and stream endpoints, all logged in `artifacts/resolver/requests.log`:

| # | Request | Result |
|---|---|---|
| 1 | iframe_api | player `1b3be681` |
| 2 | base.js | 2.9 MB |
| 3 | VISIONOS signed out, no visitor id | `LOGIN_REQUIRED` "Sign in to confirm you're not a bot" |
| 4 | TVHTML5 7.x signed out | `UNPLAYABLE` "The page needs to be reloaded." |
| 5 | TVHTML5 5.x (`tv_downgraded`) signed out | same |
| 6 | one yt-dlp run signed out (watch page, VISIONOS player, HLS manifest) | itag 251 in **2.21 s** |
| 7 | Rust, VISIONOS signed out, visitor id from a WEB_REMIX suggestions call (0.06 s) | itag 251 in **0.09 s** |
| 8 | range 0-1023 on #7's URL | **206**, 1024 bytes, 0.18 s |
| 9 | Rust, `tv_downgraded` signed in | "The page needs to be reloaded." |
| 10 | Rust, WEB_CREATOR signed in (cookies, X-Origin, visitor id) | itag 251 with `signatureCipher` and n, deciphered in **0.11 s** with the engine warm (0.63 s to load it first) |

#10's URL was not fetched, to stay within the budget. Its signature spec and n answer are among the offline values that match deno. That song offered no itag 774 on WEB_CREATOR (formats 140, 249, 250, 251).

## Decision

**What works.**
- Signed out, the Rust resolver gives itag 251 about 25× faster than yt-dlp (0.09 s against 2.2 s) and needs no JS at all.
- Signed in with Premium, WEB_CREATOR answers with ciphered formats that the embedded engine solves exactly as yt-dlp does, in about 0.1 s per song once the engine is warm (yt-dlp: 3.8–10 s per song signed in, see docs/integration.md).

**What doesn't.**
- No PO tokens, so these are out of reach:
  - `WEB_REMIX`/`WEB`/`MWEB` formats, and so far the Premium itag 774: not seen on WEB_CREATOR for the song tried; yt-dlp may get it from `web` or `web_music`, which need SABR or PO tokens.
  - Non-Premium accounts on WEB_CREATOR: URLs fail, so playback falls back to yt-dlp.
- No SABR.
- The TV clients answer "The page needs to be reloaded" for requests built from yt-dlp's defaults. They may need the `/tv` page's config; not investigated within the budget.
- VISIONOS doesn't serve "made for kids" videos.
- The first preprocessing of a new player in QuickJS takes 12–41 s, against ~1 s in deno.

**Not verified.**
- That a deciphered WEB_CREATOR URL plays: no range fetch was left in the budget.
- That VISIONOS URLs play past the first KB: the old iOS URLs failed after the first bytes. Check with one real playback before relying on it.

**Maintenance risk.**
- Client churn: four default changes in 2026 (`ios_downgraded`, `tv_embedded`, `android_vr` removed; `visionos` added). Following them means updating the client constants here, which yt-dlp users get from a yt-dlp update.
- The challenge solver is lower risk, since EJS hasn't needed a release since 2026-03. But a player change that breaks it needs a new vendored EJS, and today `yt-dlp -U` handles that.

**Size.** Dropping yt-dlp and deno saves about 40 MB (`yt-dlp_linux` 2026.08.19: 40.4 MB; `yt-dlp.exe` 17.8 MB; `yt-dlp_macos` 37.1 MB) plus deno's ~138 MB binary (41.6 MB zipped), per platform, and removes Python start-up from every resolve. QuickJS adds about 1.0 MB of code and the EJS scripts 158 KB.

**Recommendation.**
1. Keep yt-dlp as the fallback and ship the Rust resolver as an opt-in setting first.
2. Before turning it on by default:
   1. Verify one real playback of a VISIONOS stream and of a WEB_CREATOR stream (two plays).
   2. Check which client yt-dlp gets 774 from for Premium. If 774 needs `web_music` with SABR or PO tokens, the Rust path tops out at 251 (Opus ~140 kbps), against 774 (Opus ~222 kbps) from yt-dlp.
   3. Run the preprocessing at startup in the background, or port it to a Rust parser (oxc/SWC, as ytdlp-ejs does), so a player update doesn't send the next songs to yt-dlp.
3. If 774 stays out of reach, the right default for Premium accounts is yt-dlp for playback and the Rust resolver for speculation (prefetch and audition), where 251 is enough and speed matters most.
