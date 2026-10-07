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

The taste rules in docs/gpui/VISUALS.md ("Taste") apply: slow camera,
soft depth and fog, cover colours with capped chroma, nothing flashing on
the beat, readable text over the backdrop.

## Question 1: can the 3D frame reach GPUI without a CPU copy?

Today every effect renders on our own wgpu device and is read back to the
CPU and handed to GPUI as an image (crates/visuals/src/target.rs,
crates/app/src/visuals/frames.rs). That is fine for a soft 528×448 backdrop but
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
  crates/app/Cargo.toml), and CMake (`brew install cmake`; libopus builds from
  source).
- `git clone https://github.com/jvz-devx/ytfast-gpui`, then
  `cargo run --profile profiling` at the repository root (the release-speed build without LTO).
- `YTFAST_FAKE_STREAM=<an audio file>` plays a local file instead of
  YouTube, so the spike doesn't touch the account.
- Work on a branch (`spike-3d`), commit what you learn to this file, and
  keep captures out of the repository.

## Done when

This file records, with numbers: whether the zero-copy surface path works
on macOS and what it costs against readback; the two concepts tried, with
their cost and captures described; and a recommendation for M30's real
items (which concept, backdrop and visualiser, what quality settings).
