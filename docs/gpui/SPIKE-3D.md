# Spike: a procedurally generated 3D shader (PLAN M30)

To be run on the maintainer's MacBook (Apple silicon, Metal, a 120 Hz screen).
It answers two questions before any 3D effect is built for real.

## What was asked for

A procedurally generated 3D shader, used both as the Now Playing
backdrop and as a visualiser style (full window on V, and Stage). The
concepts discussed on 2026-10-07:

1. **A world per song:** the song's video id seeds a procedural landscape
   (terrain, sky, light), so every song has its own place and always the
   same one; the cover gives the colours and the music moves it.
2. **XMB in 3D:** the PlayStation 3 wave as real glossy 3D ribbons with
   soft reflections, with the fine sparkles floating through it in depth.
3. **Liquid form:** a slowly morphing glossy shape lit in the cover's
   colours; bass swells it, highs ripple its surface.
4. **Flyover:** a slow flight over a terrain whose ridges are the spectrum.

The taste rules in gpui/NOTES-visuals.md ("Taste") apply: slow camera,
soft depth and fog, cover colours with capped chroma, nothing flashing on
the beat, readable text over the backdrop.

## Question 1: can the 3D frame reach GPUI without a CPU copy?

Today every effect renders on our own wgpu device and is read back to the
CPU and handed to GPUI as an image (gpui/crates/visuals/src/target.rs,
gpui/src/visuals/frames.rs). That is fine for a soft 528×448 backdrop but
not for a sharp 3D scene at window size and 120 Hz.

On macOS GPUI draws with Metal directly (`gpui-pre-macos`, `metal` crate),
not wgpu. Its renderer can draw a `CVPixelBuffer` with the `surface`
element (Zed uses it for screen sharing). So the spike to try:

1. Create an IOSurface-backed `CVPixelBuffer` (BGRA8) at the effect's size.
2. Wrap its IOSurface as a Metal texture and import that into wgpu
   (`wgpu::hal::metal` `texture_from_raw`, then `create_texture_from_hal`),
   on a wgpu device made on the same `MTLDevice` GPUI uses if possible
   (otherwise the system default device; IOSurfaces cross devices).
3. Render a frame into it with wgpu, wait for completion (or use a shared
   event), and draw it with `gpui::surface(pixel_buffer)` in a test view.
4. Measure: frame time, CPU and GPU (Activity Monitor's GPU History, or
   `sudo powermetrics --samplers gpu_power`), at 1280×800 and full
   window, 60 and 120 fps, against today's readback path.

If it works, every existing effect can move to it on macOS; Linux
(GPUI on wgpu/Vulkan) and Windows need their own follow-up.

## Question 2: which concept, at what cost?

Prototype two of the concepts as WGSL raymarched or mesh scenes in the
visuals crate (shaders follow "Shader rules" in NOTES-visuals.md: vec4
packed uniforms), driven by the existing analysis (`Bands`, the tap's
samples) and the cover palette. For each: a capture in dark and light, the
cost at full window and 120 fps, and whether it still reads as calm behind
text at backdrop strength. Seed concept 1 from the video id so the same
song gives the same world.

## Setting up the Mac

- Xcode command line tools, Rust via rustup (`rust-version` in
  gpui/Cargo.toml), and CMake (`brew install cmake`; libopus builds from
  source).
- `git clone https://github.com/jvz-devx/ytfast-gpui`, then `cd gpui` and
  `cargo run --profile profiling` (the release-speed build without LTO).
- `YTFAST_FAKE_STREAM=<an audio file>` plays a local file instead of
  YouTube, so the spike doesn't touch the account.
- Work on a branch (`spike-3d`), commit what you learn to this file, and
  keep captures out of the repository.

## Done when

This file records, with numbers: whether the zero-copy surface path works
on macOS and what it costs against readback; the two concepts tried, with
their cost and captures described; and a recommendation for M30's real
items (which concept, backdrop and visualiser, what quality settings).

## Findings (2026-10-07, browser spike)

The maintainer asked for the spike to run in the browser with an mp3 instead of
in the app, so it answers question 2 and not question 1.

**How it was run.** `spikes/3d-web/` is a static WebGPU page (`just
spike-3d`, then http://127.0.0.1:8137/spikes/3d-web/, `?mp3=<path in the
repo>`): it plays an mp3 through Web Audio and runs the app's own analysis
on it (a port of `spectrum.rs`: 4096-point FFT, 32 bands, bass, kick,
level, at 60 hops a second) plus slow envelopes, takes the cover from the
mp3's ID3 picture with `cover.rs`'s palette and `colour_kept`, and draws
the chosen scene with a mock Now Playing (cover, title, a lyric) over it,
in backdrop or visualiser mode, dark or light. Every scene ends in the
backdrop's own tone mapping (`tone_dark`/`tone_light` from
`backdrop.wgsl`), so the backdrop numbers hold for text on top. The
uniforms follow the shader rules (vec4s only). `just spike-3d-render`
(`render.ts`, Deno's WebGPU, which is wgpu and naga on Metal) validates the
scenes, writes capture matrices (dark/light x backdrop/visualiser x a
vivid, a muted and a near-monochrome cover) and times them. The concepts
were built in parallel by subagents from one brief, then reworked with the
owner watching.

**Question 1 is still open.** The zero-copy IOSurface path can't be tried
in a browser; it needs the native test described above.

**Scenes and cost.** Wall time per frame rendered back to back (the
per-pass timestamps are unreliable on Apple GPUs: back-to-back passes
overlap, so batched timestamps cover other frames, and across submits
Metal sometimes returns stamps from another clock domain), M5 Pro, the
GPU shared with a browser tab drawing a scene at 3702x2440 (87% busy), so
these are upper bounds:

| Scene | Concept | 2560x1600 | 1280x800 | Kept |
|---|---|---|---|---|
| XMB (PS3 port) | 2 | 0.35 ms | 0.33 ms | yes |
| Spectrum ridges | 4 | 3.9 ms | 0.76 ms | yes |
| Aurora | new | 2.3 ms | 0.74 ms | yes |
| World per song | 1 + 4 | 13 ms | 3.7 ms | no |
| Liquid form | 3 | 2.9 ms | 1.1 ms | no |
| Glass | new | 2.1 ms | 0.67 ms | no |
| Nebula | new | 2.4 ms | 0.71 ms | no |

- **XMB (PS3 port)** (`xmb.wgsl`) is a port of linkev's MIT recreation
  (github.com/linkev/PlayStation-3-XMB, built from a reverse-engineering
  pass over the PS3's `spline.elf`): one 100x100 grid mesh whose rows fold
  over each other, white with a fresnel alpha from its screen-space normal,
  2000 additive point sparkles, and the PS3's diagonal night gradient in
  the cover's most colourful colour. The music sets the pace, never the
  shape: the wave flows on a clock that runs 0.6x to ~2.8x with a fast
  loudness envelope (0.08 s rise, 0.7 s fall), the sparkles on one driven
  by the highs, and the glow and sparkle light lift with those envelopes;
  the backdrop takes half of it. Three passes (gradient, mesh, sprites).
  Kept. A raymarched take from the brief
  (translucent sheets with an edge-on glow) looked less like the PS3 and
  was dropped.
- **Spectrum ridges** (`ridges.wgsl`): a slow flight over dark land whose
  ridges across the width are the 32 bands (bass in the middle), rows
  receding into haze towards a sunset in the cover's colours, a soft line
  on every ridge top. The bands become a 12-term cosine series once per
  pixel and the march hops crest to crest, which took it from ~8 ms to
  ~2 ms. Kept.
- **Aurora** (`aurora.wgsl`): two or three translucent curtains in the
  cover's colours folding over a still lake or a snow plain, striations
  shimmering with the highs; in the visualiser the curtains' height along
  their length follows the bands. Strong as a visualiser and as a dark
  backdrop; in the light look it washes out on muted covers.
- **World per song** was rebuilt from the song itself on the maintainer's
  feedback ("it just seems random"): the decoded track becomes a map
  (`songmap.js`: chroma, loudness, brightness, onsets, key and mode), the
  flight follows playback, the notes stand as ranges across the valley in
  circle-of-fifths order around the key, loudness raises the land and
  quiet passages sink into lakes, the song's character picks the biome and
  its mode the hour. The land is baked once per song into a mipmapped
  height texture (25 ms) and marched with one read a step, which took it
  from ~7 ms to ~2-3 ms at 1280x800. Its structure reads, but the maintainer
  found the look not good enough, and it costs the most.
- Liquid, Glass and Nebula work but weren't chosen.

**What carries over to the app.**

- The light look: `tone_light` presses the backdrop into luminance
  0.645-0.8, and a 3D scene loses nearly all of its shape in that range.
  The kept scenes either read as light (XMB) or are visualiser-first; a
  backdrop that must keep 3D in the light look needs a scrim behind the
  text instead of a whole-frame squeeze.
- Motion that follows the music: integrate a clock whose speed follows a
  fast envelope (never scale time by an envelope, which jumps); a blend of
  two integrated clocks stays smooth.
- Costs that only show on the target GPUs: the spike ran on Apple silicon.
  The UHD 630 notes above (integer hashes slower than `sin` there) apply
  to the ports; the ports get measured in the app.
- Chrome's WGSL compiler (Tint) rejects `a * b ^ c` without parentheses
  where naga accepts it; the spike's page compiles every scene in the
  browser as well.

**Recommendation for M30's second item.** Port XMB (PS3 port), Spectrum
ridges and Aurora as visualiser styles (full window on V and Stage), and
XMB as a backdrop style beside the current one (it reads in both looks);
Aurora as a backdrop only in the dark look. Render the raymarched ones
(ridges, aurora) at the backdrop's reduced scale with the frame-rate
setting, and measure each in the app on the UHD 630 against the GPU
budget above before it ships.
