# Download size (M26)

Measured 2026-10-07 on the dev box (Fedora 43, 6 cores, Rust 1.99), Linux
x86_64, with other agents building at the same time. Sizes are bytes of the
stripped release binary and of the packages built from it the way
.github/workflows/release.yml builds them.

## Release profile

| Variant | Binary | .text | Rebuild after one edit |
|---|---|---|---|
| Before: thin LTO, opt-level 3 | 65.4 MB | 45.4 MB | 219 s |
| Fat LTO, 1 codegen unit, opt-level 3 | 50.0 MB | 36.7 MB | 409 s |
| **Fat LTO, 1 codegen unit, opt-level "s", hot crates at 3** | **37.5 MB** | 25.1 MB | 283 s |
| The same, skrifa deduplicated (shipped) | 37.4 MB | | |

"Rebuild after one edit" is `touch crates/app/src/main.rs` and `cargo build --release
-j 3`: the bin crate plus the LTO link. A cold build of the shipped profile
took 7 min at `-j 3` without the rustc wrapper (`RUSTC_WRAPPER=`).

Hot crates kept at opt-level 3 (`[profile.release.package.*]` in
crates/app/Cargo.toml): symphonia and its codec and format crates, the libopus
adapter and opusic-sys (libopus is C; cc takes cargo's opt-level), rubato,
realfft, rustfft, audioadapter*, cpal, encore-audio, encore-visuals, wgpu,
wgpu-core, wgpu-hal, wgpu-types, naga and gpui-pre-wgpu (GPUI's renderer,
which draws the effects' frames). The 2026-10-08 Now Playing CPU follow-up
also keeps gpui-pre's scene ordering and cached-view replay at 3; see
VISUALS.md for the alternating profiling measurements. The release sizes
above predate that addition. Generic code is instantiated in the crate
that uses it, so these crates' uses of core, alloc and hashbrown stay at 3
too. Panics still unwind. opt-level "z" wasn't tried: "s" already met the
CPU bar, and each variant is a full rebuild.

The `profiling` profile inherits release, overrides included (checked with
`cargo build --profile profiling -v`: anyhow and log at `opt-level=s`,
encore-audio, rubato and symphonia-core at 3). Only LTO and codegen units
differ, so its CPU numbers stand for release builds.

## CPU

`scripts/gpui-measure.sh` (app CPU in % of one core over 10 s), signed out
in fresh XDG dirs, `ENCORE_FAKE_STREAM` with the test audio, a song opened
over MPRIS, then Now Playing with the effects on; three samples per state,
1280x1000 window, release binaries.

| Variant | Playing, Home | Now Playing |
|---|---|---|
| Before (thin LTO, O3), run 1 | 4.0-4.2% | 6.7-6.9% |
| Before, run 2 | 4.2-4.4% | 6.9-7.5% |
| Fat LTO, O3 | 3.8-3.9% | 7.3-7.7% |
| Fat LTO, "s" + hot at 3, run 1 | 4.1-4.6% (one 9.1% outlier with desktop GPU at 37%) | 7.3-7.4% |
| Same, run 2 | 3.8-4.4% | 6.7-7.0% |
| Shipped (skrifa deduplicated) | 4.2-4.4% | 7.1-7.4% |

All within the spread of the two baseline runs. Memory (PSS) went from
160-180 MB to 145 MB playing on Home.

## Installers

Linux packages built locally from the shipped binary; "before" is each
tool's previous setting with the same binary.

| Package | Before | After | Setting |
|---|---|---|---|
| AppImage | 15.67 MB | 14.41 MB | zstd level 19, 1 MB blocks (was zstd defaults, 128 KB) |
| .rpm | 12.30 MB | 11.23 MB | xz 9 payload (was zstd 19) |
| .deb | 11.52 MB | 11.52 MB | unchanged: cargo-deb's xz |
| Windows zip (Linux binary as a stand-in) | 15.59 MB | 15.44 MB | `7z -mx=9` |
| dmg | | with the next release | ULFO (LZFSE), was UDZO (zlib) |
| setup.exe | | with the next release | `lzma2/ultra64`, was `lzma2` (solid already) |

With the old profile (65.4 MB binary) the AppImage was 23.97 MB, the rpm
17.59 MB and the deb 16.36 MB: together with the profile, the AppImage is
40% smaller and the rpm 36%.

Notes:

- appimagetool's own mksquashfs only has zstd (`--comp xz` fails), and the
  type 2 runtime reads it the same way. 1 MB blocks are what matter: level
  19 alone gave 1% and 1 MB blocks at the default level 1%, both together
  10%. Start cost: `--version` from the AppImage 50 ms instead of 30-40 ms;
  reading the whole binary out of the mounted image cold 0.14 s instead of
  0.13 s. The M16 updater swaps AppImage files by rename and probes
  `--version`; it doesn't look inside the squashfs.
- The dmg: the cask asks for macOS 11 (Big Sur) and Info.plist for the
  binary's minimum (11.0 on arm64, 10.12 on x86_64); ULFO needs 10.11. The
  updater mounts the image with `hdiutil attach -readonly`, which reads any
  format.
- The .deb: cargo-deb has no level option (`--fast` only makes it worse).
  Recompressing data.tar with `xz -9e` would save 2.6% (11.22 MB) but means
  repacking the ar archive by hand; not done.
- The zip stays deflate so Explorer opens it; LZMA (7z) would be 40%
  smaller but needs a separate tool on Windows.

## Duplicate dependencies

`cargo tree -d -e normal` in gpui/ for Linux, Windows and macOS, and in the
root crate (`--no-default-features`).

Removed: skrifa 0.44, read-fonts 0.41 and font-types 0.12. swash 0.2.10
allows skrifa `>=0.31.1, <=0.44`; cosmic-text 0.19 needs 0.40, so
`cargo update skrifa@0.44.0 --precise 0.40.0` gives one set.

Left, and why (on Linux unless noted):

- Pinned by gpui-kit 0.7.1 and its gpui-pre set: hashbrown 0.14/0.15/0.16
  (zed-xim, gpu-descriptor, wgpu 29) next to 0.17, itertools 0.11/0.13/0.14,
  rustc-hash 1 (naga and wgpu-core 29), png 0.17 (tiny-skia 0.11, whose 0.12
  needs a newer resvg), roxmltree 0.20 (fontconfig-parser 0.5.8, the latest),
  dirs 5 (zed-font-kit), pollster 0.2 (postage), phf 0.11, spin 0.9,
  thiserror 1, toml 0.8 (rust-i18n). Windows: resvg/usvg 0.45 (gpui-component)
  next to 0.46. macOS: the cocoa/core-graphics/objc2 generations used by
  gpui-pre's platform crates.
- souvlaki 0.8.3 (the latest) needs windows 0.44, next to gpui's 0.57-0.62.
- sha2 0.10 (ours, the backend's and oo7's) next to 0.11 (rust-embed-utils):
  moving ours to 0.11 leaves oo7 on 0.10.
- miniz_oxide 0.8 (png, backtrace) next to 0.9 (flate2 1.1.10), and base64
  0.22 next to 0.23 (hyper-util 0.1.21): each would need a downgrade of
  flate2 or hyper-util, not a bump.
- getrandom 0.2/0.3/0.4 (ring, rand 0.9 in gpui-pre, ashpd/uuid).
- Root crate: hashbrown 0.15 through rusqlite 0.37's hashlink 0.10
  (rusqlite 0.40 moves to hashlink 0.12, a breaking upgrade of the cookie
  reader; and gpu-descriptor keeps 0.15 in the app anyway) and the base64
  pair above.
