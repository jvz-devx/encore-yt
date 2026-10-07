# M8 and M9 visuals

How the effects are drawn: the player bar's shaders (M9), the layers behind
Now Playing (M8), then the spike's findings (how to get wgpu output into the
GPUI window, and the audio mpv plays), which the code still follows.

## Player bar (M9)

Code: `crates/visuals/src/strip.rs` + `shaders/strip.wgsl`,
`dissolve.rs` + `shaders/dissolve.wgsl`, `gpu.rs` (the one device every
effect shares), `target.rs` (the readback ring, and `frame_now` for still
pictures); app side `src/visuals/bar.rs`, `dissolve.rs`, `effects.rs`.

- **One strip, one pass**: the bar's whole background is one
  `Strip` frame at device size (1280x88 here), painted by the effects layer
  under the app. The player bar leaves out its `base` fill and gives the
  kit slider transparent colours while a frame shows
  (`visuals::paints_bar`), so the slider still takes clicks and drags.
  Slot boxes (`Slot::Bar`, `Seek`, `Play`, `BarCover`) tell the shader where
  the controls are.
- **Glow**: four soft blobs of the cover's palette drift along the bar,
  nothing at its top edge and fullest at the window's bottom edge, breathing
  with a slow envelope of the kick and bass. It is applied in OKLab: chroma
  plus at most +0.05 lightness in the dark look (relative luminance capped
  at 0.0085, so `text_faint` keeps 4.5:1), only lighter in the light look.
  A test renders red, blue and white palettes in both looks and checks it.
  New covers cross-fade over 1.2 s.
- **Seek bar**: the fill keeps the kit track's top edge and hangs below it as
  deep as the song is loud (the M8 waveform, 512 texels), lit along the top
  and shaded towards the loudness edge; unplayed is `text` at 16-20%. A soft
  `signal` bloom around the played part, and an ink playhead that swells on
  the kick with a `signal` ring pulsing out of it (larger under the
  pointer). **The M6 ridge** rises from the same top edge, upwards, in the
  app layer, so it draws over the strip without any coordination: the
  loudness hangs below the line, the replay heat rises above it.
- **Halos**: rings 2 points outside the play button and the cover, in the
  palette's most colourful entry, swelling and brightening on the kick,
  with a soft spill on the breath.
- **Dissolve**: on a track change the old cover's image stays over the
  slot (no placeholder flash) until the new one has loaded, then burns into
  it over 0.9 s along a three-octave noise front with an edge in the new
  accent; rendered at the cover's device size (56 or ~400 px) and painted
  over the app by the shell. In the bar it is skipped while Now Playing is
  open (the cover is the close button then), in Now Playing while the
  cover flies. None under reduced motion.
- **Pacing**: every frame redraws the whole window, about 2 ms of CPU here
  even with the app view cached (measured: 30 frames a second with no
  strip and no tap cost 10.7% against 4.1% without effects). So while only
  the bar moves, the ticker wakes the window only when it would look
  different: a kick or bass step of 0.06 (at most 20 fps), half a device
  pixel of playhead, or the drift at 6 fps. Now Playing's backdrop keeps a
  steady 30. A still picture (paused, reduced motion, a seek) is rendered
  and waited for in the same render (`frame_now`), so it costs no extra
  window redraws.
- **Stopping**: nothing while paused or minimised; under reduced motion
  only the playhead moves (a still frame when it moved half a pixel). The
  device is dropped 30 s after the last frame of any effect; the last strip
  frame stays on screen. Stage hides the bar, and the strip with it.

Measured 2026-10-07, release build, 1280x1000 window, Home page, % of one
core from `top` over 10 s (the machine shared with other agents):

| State | app CPU | RSS |
|---|---|---|
| main before M9, playing, Now Playing closed | 4.6% | 179 MB |
| effects off (`YTFAST_GPUI_VISUALS=0`), playing | 4.1% | 164 MB |
| playing, strip at a steady 30 fps (first cut) | 13.5-14% | 176-206 MB |
| same, no strip and no tap (GPUI's redraw alone) | 10.7% | 170 MB |
| **playing, paced (shipped)**, red cover, dark | 7.9-8.1% | 210 MB |
| same, blue cover, light | 8.3% | 203 MB |
| reduced motion, playing | 4.0% | 208 MB |
| paused | 0.0% | 266 MB |
| minimised, playing | 0.0% | 266 MB |

The 6% budget isn't met: the app alone is 4.1% (it re-renders all its views
5-6 times a second on playback events and the 500 ms clock), the tap 1.3%,
and the paced strip about 3% (about 9 window redraws a second on a song
with a strong beat). The biggest lever left is outside the effects: give the
player bar's clock and slider their own entity, so a playback event doesn't
re-render the whole app.

Captures (`artifacts/gpui/`, gitignored): `m9d-sheet` (dark, red then blue
cover, pairs 1 s apart), `m9l2-sheet` (light), `m9d-diss1-zoom` and
`m9d-diss-sheet` (the bar's cover mid-burn), `m9n-sheet` (Now Playing's
cover mid-burn), `m9r-sheet` (reduced motion pair, then paused),
`m9-home-bar` (paused, light, the ridge over the waveform). Songs were
opened over MPRIS (`playerctl -p ytfast open https://music.youtube.com/watch?v=…`).

## Now Playing (M8)

Code: the crate `gpui/crates/visuals` (`ytfast-visuals`, no GPUI) and the
app side in `gpui/src/visuals/`.

- **Crate**: `Renderer` (on the shared `Gpu`, `shaders/backdrop.wgsl`,
  offscreen target and readback; `frame(params)` returns BGRA bytes), `Cover` (48x48
  pre-blurred upload and a four-colour palette), `AudioTap` (PipeWire tap,
  FFT, 32 bands plus bass, kick and level) and `waveform` (ffmpeg decode of
  the URL mpv plays, 400 values, cached per video id in the cache
  directory). `scripts/check.sh visuals` checks and tests it, `shaders`
  validates the WGSL with naga.
- **Layers** (`visuals::shell`, what `MusicApp` renders): `Effects` under
  the app (backdrop over the page panel and the spectrum strip), `Content`
  (the app's views, an `AnyView::cached` entity while Now Playing shows) and
  `Flight` over it (the cover flying between the player bar and Now
  Playing, `motion::SLOW`, ease-out). `Effects` notifies itself from a
  30 fps timer; the notify marks `MusicApp` dirty too, but its render is
  only the shell, the app's views come from the cache. Mouse and key input
  drop the cache for one frame (a slider drag changes a model, not a view).
- **Backdrop**: the cover blurred, domain-warped and turning slowly over a
  gradient of its palette, a soft bloom of its brightest colours, motes
  (particles) drifting up and flaring on the beat, a bass pulse, dither.
  New covers cross-fade over 1.2 s. Tone-mapped per theme so text keeps
  4.5:1: dark look capped at luminance 0.045 (measured 0.022-0.042 on a red
  cover), light look pressed into 0.645-0.8 (measured 0.64).
- **Spectrum**: 64 bars mirrored around the centre (lows in the middle)
  above the title, painted with GPUI quads in `text` at 28-88% opacity.
- **Waveform**: under the title, thin bars, the played part in the accent
  colour; a click seeks. A faint line until the decode (1.1-1.3 s) is done.
- **Stopping**: no frames and no tap when Now Playing is closed, the window
  is hidden (minimised), playback is paused or motion is reduced (then one
  still frame per cover, no spectrum, no particles, no flight). The renderer
  (the second Vulkan device) is dropped 30 s after Now Playing closes.
  Reduced motion is `theme::reduced_motion` (the desktop portal) or
  `YTFAST_GPUI_REDUCED_MOTION=1`.
- **Settings** (env): `YTFAST_GPUI_VISUALS=0` off, `YTFAST_GPUI_VISUALS_FPS`
  (30), `YTFAST_GPUI_VISUALS_UNCACHED=1` (no cached app view, to measure
  it), `YTFAST_GPUI_VISUALS_FLIGHT_MS` (slow the flight down to look at it).

Measured 2026-10-07, release build, 1280x1000 window, backdrop rendered at
528x448 (load 0.7-1.7; CPU in % of one core over 5-8 s, from
`/proc/<pid>/stat`):

| State | app CPU | RSS |
|---|---|---|
| Home, paused | 0% | 148 MB |
| Now Playing, paused | 0% | 167 MB |
| Now Playing, playing, 30 fps | 9-10% | 171-175 MB |
| same, app view uncached (before) | 19% | 177 MB |
| same, 60 fps | 18% | 150 MB |
| Now Playing closed, playing | 3% | 175 MB |
| minimised, playing | 2% | 150 MB |
| reduced motion, playing | 2% | 177 MB |

Frame cost at 30 fps: submit 0.27 ms, wait 0.01 ms, copy 0.15 ms. 60 fps
doubles the CPU for little visible gain on a slow backdrop, so 30 stays the
default. Not done: Stage has no backdrop yet (Stage belongs to M6; it can
place `visuals::slot` boxes the same way), a window covered by another one
wasn't tested, and the frame rate doesn't switch to 60 on its own.

# The spike (2026-10-06)

Measured on 2026-10-06: Fedora 43, KDE Plasma 6 Wayland,
Intel UHD 630 (Vulkan), 1920x1080 at **120 Hz**, gpui-kit 0.7.1 / gpui-pre
0.3.8 / wgpu 29.0.4, release build. The machine was shared with other agents'
builds (load 3 to 8 for the numbers below, 20+ for the first runs), so CPU
figures are ±3 points. The spike ran with `YTFAST_GPUI_VISUALS_SPIKE=1`
(removed since) and drew everything into the page area; its file names
below (`gpu.rs`, `spike.rs`) are now `renderer.rs` and `effects.rs`.

## 1. wgpu output inside the GPUI window

**Chosen: our own wgpu device renders offscreen; each frame is read back and
painted as a GPUI image** (`Window::paint_image` with an `Arc<RenderImage>`).

What gpui-pre 0.3.8 offers, read from the sources:

- `PaintSurface` / `Window::paint_surface` / the `surface()` element exist,
  but only on macOS (`CVPixelBuffer`). The wgpu renderer has a YUV
  `surfaces` pipeline and shader, yet draws nothing for
  `PrimitiveBatch::Surfaces` ("macOS-only for video playback",
  `gpui-pre-wgpu/src/wgpu_renderer.rs` around line 1612).
- GPUI's device lives in `gpui_wgpu::GpuContext` (`Rc<RefCell<Option<WgpuContext>>>`)
  inside the Wayland/X11 client state. Nothing public reaches it from
  `App` or `Window`, so we can't share the device or hand GPUI a texture.
- No custom shader or custom primitive API. `runtime_shaders` is the Metal
  backend's shader compilation, not a user hook.
- `canvas()` + `window.paint_image(bounds, image_bounds, radii, Arc<RenderImage>, 0, false)`
  works. A `RenderImage` is BGRA (premultiplied), the atlas format is
  `Bgra8Unorm` on this GPU, so the readback bytes go in unchanged. Each new
  `RenderImage` has a new id and is uploaded into GPUI's sprite atlas;
  `window.drop_image` frees the tile. The atlas sampler is linear, so a small
  frame scales up smoothly.

How the prototype does it (`visuals/gpu.rs`):

- A second wgpu device (`LowPower`, Vulkan) with a full-screen-triangle
  pipeline (`backdrop.wgsl`): domain-warped, slowly turning copy of the cover
  (uploaded at 48x48, blurred further with 13 taps), mixed with a gradient of
  the cover's four quadrant colours, a bass pulse from the spectrum, a
  vignette and dither against 8-bit banding.
- Two render target + readback buffer slots. Frame N is submitted while frame
  N-1 (already finished) is mapped and copied: one frame of latency, and the
  wait for the GPU is ~0.01 ms.
- The previous image is dropped from the atlas one frame late, so a texture
  that holds only our frame isn't freed and re-created every frame.
- A timer task notifies the view at the target rate while playing.
  `request_animation_frame` would draw at the display rate (120 Hz here).

Measured (the overlay line, averaged over 2 s; CPU as % of one core from
`/proc/<pid>/stat`, 5 s windows):

| Setup | fps | submit | wait | copy | app CPU |
|---|---|---|---|---|---|
| no visuals, playing (baseline) | | | | | 3% |
| 640x360, paced at 30 | 30 | 0.33 ms | 0.01 ms | 0.24 ms | 15% |
| 640x360, paced at 60 | 56-58 | 0.42 ms | 0.02 ms | 0.23 ms | 25% |
| 640x360, every display frame | 61-74 | 0.39 ms | 0.01 ms | 0.26 ms | 29% |
| 1280x720, paced at 60 | 58-60 | 0.27 ms | 0.01 ms | 0.56 ms | 30% |
| 64x36 at display rate (load 20) | 36-45 | 0.45 ms | 0.02 ms | 0.01 ms | 13% |
| paused (any setup) | 0 | | | | 2% |
| minimised while playing | 0 to 2 | | | | 2-3% |

- Our share of a frame is about 0.7 ms at 640x360 (submit + copy, plus
  GPUI's atlas upload of 0.9 MB) and 1 ms at 1280x720. **Most of the ~3.8 ms
  CPU per frame is GPUI re-rendering the whole window**: `cx.notify` on a
  view marks every ancestor view dirty (`Window::mark_view_dirty`), and
  `MusicApp` is one entity whose `render` builds the sidebar, page, queue and
  player bar each time. The 64x36 run shows the same CPU as 640x360.
- RSS grows from ~115-145 MB to ~195-240 MB with the spike: the second
  Vulkan device, its pipeline and buffers.
- A blurred backdrop looks the same at 640x360 as at 1280x720 (compare
  `m8-640-a.png`, `m8-1280-a.png`), so render small and let GPUI scale.
- Paused: no frames (`advance` returns early), the ticker task is dropped.
  The 2% is the PipeWire linker polling `pw-dump` once a second (see below).
- Minimised: KWin sends no frame callbacks, GPUI doesn't draw, so `render`
  and the GPU work stop on their own. Not tested: a window fully covered
  by another one.

Captures (`artifacts/gpui/`, gitignored): `m8-640-a.png` and `m8-640-b.png`
one second apart (the flow moved, mean pixel difference 1.7%),
`m8-1280-a.png`, `m8-fps60-paused.png` (frozen backdrop, bars down, waveform
with progress), `m8-wave-a.png`, and `m8-final-a.png`/`m8-final-b.png`
(final code: 57-59 fps, submit 0.25 ms, copy 0.15 ms; no PulseAudio source
outputs while tapping).

Rejected:

- **Patching gpui-pre** to share its device and draw an external texture
  (fill in `PrimitiveBatch::Surfaces` for wgpu with an RGBA variant, give
  `PaintSurface` a non-macOS payload, expose the `WgpuContext` through
  `PlatformWindow`). It is the zero-copy option, about 200-300 lines across
  gpui-pre, gpui-pre-wgpu and gpui-pre-linux plus `[patch.crates-io]`. But
  AGENTS.md says not to vendor or patch upstream crates, and a fork has to
  move with every gpui-kit bump. Worth proposing upstream; until then the
  copy costs under 1 ms a frame.
- **A Wayland subsurface of our own** under or over GPUI's surface, with its
  own wgpu surface (`Window` implements `HasWindowHandle`). Zero-copy too,
  but needs our own wl_subsurface on GPUI's `wl_display`, a transparent GPUI
  window where the effect shows through, frame sync between two
  renderers, and a separate X11 path. Too much machinery for a background.
- **Drawing the effects with GPUI primitives only** (gradients, quads,
  paths). Fine for the spectrum bars and the waveform (the prototype does
  that), not for a warped, blurred cover or bloom.

## 2. Audio samples for analysis

**Chosen: a PipeWire tap on mpv's playback stream**, through the PipeWire
command line tools (`visuals/pipewire.rs`, `visuals/spectrum.rs`).

- mpv's stream is the node named `ytfast` (`--audio-client-name=ytfast`,
  media class `Stream/Output/Audio`).
- `pw-record --raw --format f32 --rate 48000 --channels 2 --latency 10ms
  --target 0 -P '{ node.name = ytfast-visuals-<pid> media.class =
  Stream/Input/Audio/Analyzer node.dont-reconnect = true }' -` opens an
  unlinked capture node and writes PCM to its stdout.
- A linker thread runs `pw-dump` once a second and `pw-link`s the FL/FR
  output ports of every `ytfast` node to our input ports. With Smooth mixes
  or Audition both decks mix into the tap; a new mpv process (a restart)
  gets linked within a second.
- The media class matters. With the default `Stream/Input/Audio`, KDE shows
  the "microphone in use" icon in the tray while the visualizer runs, and
  WirePlumber may move the stream to the microphone if mpv goes away.
  `Stream/Input/Audio/Analyzer` is left alone by WirePlumber and not listed by
  pipewire-pulse (`pactl list source-outputs` is empty), so there is no
  icon: `m8-tray-both.png` (top: plain stream, bottom: analyzer class).
  Plasma's indicator skips only "virtual" source outputs, those without a
  client (pulseaudio-qt `stream_p.h`).
- Analysis: 800-sample hops (1/60 s), Hann window, 2048-point FFT, 32
  log-spaced bands 40 Hz to 16 kHz, dB with a 48 dB range, automatic gain
  (mpv applies its volume before PipeWire sees the samples), fast attack,
  slow release. The log shows 60 frames a second, for example
  `spectrum frame 241: ▅▅▅▇▇▇▆▆▆▅▅▅▄▄▄▄▃▂▄▃▃▂▃▃▃▃▃▃▃▃▂▂`.
- Cost: pw-record under 1% of a core; the FFT thread is inside the app's
  number and wasn't measured on its own. Paused: mpv's stream stops, nothing flows,
  the reader sleeps on the pipe.
- Latency: the tap gets each quantum as it goes to the sink; the FFT window
  centre is ~21 ms old. Not measured against the speakers.

Rejected:

- **The `pipewire` crate** (libpipewire bindings). The proper version of the
  above (a `pw_filter`, registry events instead of polling, no child
  processes), but it needs `pipewire-devel` and clang at build time, which
  this machine doesn't have. Move to it when adding the dependency is
  agreed; the linking and media class tricks carry over.
- **mpv lavfi filters** (`af=lavfi=[asplit…]` with `astats`/`aspectralstats`
  and `ametadata=print:file=…`, or `af-metadata` over IPC). `aspectralstats`
  gives centroid/flux/rolloff, not bands, so 32 bands need 32 `bandpass` +
  `astats` branches; the filter runs ahead of the speakers by mpv's audio
  buffer (~0.2 s) and needs pts matching; and it shares `af` with the
  equalizer and loudness levelling. More fragile for a worse result.
- **Capturing the sink monitor**: picks up every app's sound, and shows the
  microphone icon.

### Whole-song waveform

**Chosen: decode the stream URL with ffmpeg at 8 kHz mono** in the
background and keep 400 peaks (`visuals/waveform.rs`). The spike asks mpv
for `path` over a second IPC connection; in the app the backend should do
this, since it owns the resolved URL.

- Amor Factura (237 s): 1.49 s wall in the app, 1.4 s user CPU from the
  shell. Mientras tanto (330 s): 2.0 s. Memory: ~90 MB peak RSS in ffmpeg,
  7.6 MB of f32 samples in our process before reducing to peaks.
- It downloads the song a second time (~4 MB for itag 251). ffmpeg's ranged
  reads came through in ~2 s; a plain `curl` of the same URL was throttled to
  118 s.
- Plain peaks look like a solid block for loud masters (`m8-wave-a.png`):
  use RMS per bucket, or peaks on a log scale.

Rejected or not working:

- **mpv `dump-cache`** to reuse mpv's cached bytes: with a fresh `--ao=null`
  mpv the whole song was cached after 1 s, but the dump came out truncated
  (3.75 MiB, `File ended prematurely`, 230 of 237 s) even when given 10 s.
  In the app the cache also starts at the restored position, so
  `bof-cached` stays false. Not reliable as tested.
- **symphonia**: no Opus decoder, and YouTube Music serves Opus (itag 251)
  first.
- **A second mpv with `--ao=null`** playing at full speed: more moving parts
  than ffmpeg for the same decode.

## 3. Constraints

- **Frame pacing**: the display runs at 120 Hz; `request_animation_frame`
  follows it. Pace effects with a timer (30 Hz for a slow backdrop, 60 Hz
  for the visualizer) and stop the timer when nothing moves.
- **Whole-window re-render**: any animating view re-renders `MusicApp`.
  Before shipping 60 fps effects, make the big areas (sidebar, page, queue,
  player bar) their own entities rendered with `AnyView::cached(...)`, so an
  animation frame re-renders only itself. This is the largest lever on CPU.
- **Hidden/paused**: minimising stops drawing (no frame callbacks). Pause
  stops the timer. Covered windows not tested. The `pw-dump` poll costs ~2%
  while idle; use `pw-dump --monitor` or the pipewire crate's registry
  events.
- **Reduced motion**: GPUI has `App::reduce_motion()` /
  `set_reduce_motion()` but the Linux platform never sets it. The desktop
  portal has it: `org.freedesktop.portal.Settings.ReadOne
  org.freedesktop.appearance reduced-motion` returns `u 0` here
  (xdg-desktop-portal 1.20, KDE backend). Read it at start with zbus (the
  backend already depends on it), watch `SettingChanged`, and call
  `cx.set_reduce_motion`. The spike stops animating when it is set (from the code;
  not tested, since nothing sets it yet).
- **Intel iGPU**: the shader at 640x360 is no trouble; GPU time wasn't
  measured (no `intel_gpu_top` without root). A second Vulkan device costs
  ~50-90 MB RSS. If `Backdrop::new` fails, the spike shows the error and the
  rest keeps working (from the code, not tested); the real effects should fall back to a GPUI gradient.
- **Wayland/KDE**: offscreen rendering needs no surface, so nothing differs
  between Wayland and X11 for the backdrop. The PipeWire tap needs
  `pw-record`, `pw-link` and `pw-dump` (pipewire-utils on Fedora).

## Plan for the M8 items

1. **Animated album-art background** (Now Playing): move `gpu.rs` and
   `backdrop.wgsl` into a `visuals::backdrop` entity drawn behind the Now
   Playing view, 640x360 (or the page size / 2, capped), paced at 30 fps.
   Cross-fade covers on song change in the shader (two cover textures and a
   mix uniform). Colour: the four quadrant averages, lifted in saturation;
   check text contrast against the theme.
2. **Blur, bloom, gradients**: more passes on the same device: downsample
   chain plus a separable Gaussian for the blur, a bright-pass and add for
   bloom; all at 1/2 or 1/4 size. Gradients in the shader, or GPUI's
   `linear_gradient` where it's a plain fill.
3. **Visualizer**: keep the PipeWire tap and FFT in a backend-side module
   (GPUI-free), publish bands through a shared buffer; draw bars with GPUI
   quads (as now) or in the shader for glow. Swap the hand-written FFT for
   `realfft`, use a 4096-point FFT or start at 60 Hz (the lowest bands now
   share FFT bins). Hide or freeze it under reduced motion.
4. **Waveform on the seek bar**: backend command that ffmpeg-decodes the
   resolved URL after playback starts, cached per video id in the cache
   directory (400 RMS values); the seek bar paints them with `canvas` +
   `paint_quad`, played part in the accent colour.
5. **Particles**: a small instanced-quad pass in the backdrop shader
   (positions from a time-seeded hash, nudged by the bass), so they cost no
   extra readback. Off under reduced motion.
6. **Now Playing transitions** (cover flying from the player bar): GPUI
   animations (`with_animation`, absolute-positioned `img` with animated
   bounds) are enough; wgpu adds nothing here. They respect
   `reduce_motion` already.
7. **Idle cost**: cached views first (see constraints), then timers that
   stop on pause, hide and reduced motion; replace the `pw-dump` poll.
8. **Long term**: propose an external-texture primitive for gpui-pre's wgpu
   renderer upstream; it would remove the readback and the second device.

Code pointers: `gpui/crates/visuals/src/renderer.rs` (device, pipeline,
readback ring, cross-fade), `shaders/backdrop.wgsl`, `cover.rs`,
`pipewire.rs` (tap and linking), `spectrum.rs` (FFT and bands),
`waveform.rs` (mpv IPC, ffmpeg, cache); app side `gpui/src/visuals/mod.rs`
(shell, settings), `effects.rs`, `content.rs`, `flight.rs`, `slots.rs`,
`waveform.rs`.

## Shader rules

- Buffers hold only `vec4<f32>`/`vec4<u32>` fields (and arrays of them), or
  scalars with no `vec3` in front of them. Never pack a scalar into a
  `vec3`'s fourth slot (`pos: vec3f, size: f32`): Adreno's Vulkan driver
  (Android) misreads every field after such a `vec3` in read-only storage
  buffers in the vertex stage, with no validation error, while desktop GPUs
  read it fine (seen in another project, 2026-10-07). Put the scalars in
  `.w` of a `vec4` and unpack them in the shader, as every `Params` struct
  here already does. For particle data in a storage buffer, use a packed
  struct of `vec4f`s with `pack_`/`unpack_` helpers at the point of access.
- Every `.wgsl` validates with naga (`scripts/check.sh shaders`).
