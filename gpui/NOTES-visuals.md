# M8 and M9 visuals

How the effects are drawn: the player bar's shaders (M9), the layers behind
Now Playing (M8), then the spike's findings (how to get wgpu output into the
GPUI window, and the audio mpv plays), which the code still follows.

## Player bar (M9)

Code: `crates/visuals/src/strip.rs` + `shaders/strip.wgsl`,
`dissolve.rs` + `shaders/dissolve.wgsl`, `gpu.rs` (the one device every
effect shares), `pipelines.rs` (every effect's pipeline and the pipeline
cache), `target.rs` (the readback ring, and `frame_now` for still
pictures); app side `src/visuals/bar.rs`, `dissolve.rs`, `effects.rs`,
`device.rs` (the device made in the background).

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
  pointer). **The M6 ridge** rises from the same top edge, upwards: the
  loudness hangs below the line, the replay heat rises above it. While the
  strip paints the bar, the strip draws the ridge too (the heat as a second
  512-texel texture, blended in display space like GPUI's paths) and the
  ridge view keeps only its peak mark; without effects the ridge view
  draws it with GPUI paths.
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
- **Pacing**: every frame redraws the whole window, about 2 ms of CPU and
  7-10 ms of GPU time here even with the app view cached (measured: 30
  frames a second with no strip and no tap cost 10.7% CPU against 4.1%
  without effects; GPU in "GPU budget" below). So the strip draws only
  when it would look different: a kick step of 0.12 or a bass step of 0.24
  (at most 10 fps), a device pixel of playhead, or the drift at 3 fps.
  While only the bar moves, the ticker wakes the window only then; inside
  Now Playing's frames (20 fps) the strip keeps the same rule. A still picture (paused, reduced motion, a seek) is rendered
  and waited for in the same render (`frame_now`), so it costs no extra
  window redraws.
- **Stopping**: nothing while paused or minimised; under reduced motion
  only the playhead moves (a still frame when it moved half a pixel). The
  device is dropped 30 s after the last frame of any effect (and made again
  in the background when needed, "Shader warm-up" below); the last strip
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

The 6% budget wasn't met then: the app alone was 4.1% (it re-rendered all
its views 5-6 times a second on playback events and the 500 ms clock), the
tap 1.3%, and the paced strip about 3% (about 9 window redraws a second on a
song with a strong beat).

### Position ticks redraw only the player bar (2026-10-07)

- Playback reports that only move the position (most of them, four a
  second) and the 500 ms clock no longer notify `MusicApp`; they notify
  `playback::Clock`, which the player bar and the mini player watch. Stage
  and Now Playing (its waveform) still redraw the app on each tick.
- The player bar is its own cached view (`views::PlayerBar`), laid out by
  the shell under the app's views in the room `player::space` leaves, so
  dialogs, menus and the Play anything scrim still cover it. GPUI dirties a
  notified view's ancestors, so a bar inside the app's views would have
  re-rendered all of them. Both views are cached in every state now (before
  only while an effect animated), still rendered afresh after input.
- While the strip draws the seek bar, a tick asks for no frame at all: the
  shell compares what the bar shows of the position (the elapsed second, the
  ridge's playhead pixel) with what it last drew and renders the bar afresh
  in the next effects frame. Under reduced motion the effects layer is
  woken for that frame. The seek slider is moved as the bar renders (a
  slider change asks for a frame of its own).
- The tap's FFT computed `sin_cos` in every butterfly (about a quarter of
  the app's CPU in a profile); the twiddle factors are now a table.
- Counted over 5 s on Home, playing: 74-75 window frames, all of them
  effects frames (before: about 85, of them 10 not), the app's views
  rendered 0 times, the bar 34-35 times.

Measured with the test audio (`YTFAST_FAKE_STREAM`, the same song for
both), release builds, 1280x1000 window, Home, % of one core from `top`
over 10 s, `before` = main at the merge, `after` = this change. The machine
was shared with other agents' builds, so each pair ran back to back; the
low-load pairs (load average 1.3-1.8) are the cleanest:

| State | before | after |
|---|---|---|
| playing, Now Playing closed, low load (two pairs) | 9.7%, 9.7% | 5.6%, 5.6% |
| same, load 2-13 (four pairs) | 11.3-13.5% | 5.7-6.8% |
| effects off (`YTFAST_GPUI_VISUALS=0`), playing | 4.5% | 1.8% |
| reduced motion, playing | 4.6-7.4% | 1.7-2.5% |
| paused | 0.0-0.1% | 0.0-0.1% |
| minimised, playing | 0.2-2.4% | 0.1-1.5% |
| Now Playing open, playing | 14.7-16.8% | 14.4-17.4% |

Now Playing is unchanged: its waveform shows the position, so each tick
still re-renders the app; its backdrop runs at a steady 30 fps anyway.
Captures: `v-bars` (0:05 then 0:08, then a click mid-bar seeked to 1:32 of
3:00), `v-sheet` (Up next panel, an album page, Play anything's scrim over
the bar, Now Playing), `ro-bars` (reduced motion and effects off, the
position moving).

Captures (`artifacts/gpui/`, gitignored): `m9d-sheet` (dark, red then blue
cover, pairs 1 s apart), `m9l2-sheet` (light), `m9d-diss1-zoom` and
`m9d-diss-sheet` (the bar's cover mid-burn), `m9n-sheet` (Now Playing's
cover mid-burn), `m9r-sheet` (reduced motion pair, then paused),
`m9-home-bar` (paused, light, the ridge over the waveform). Songs were
opened over MPRIS (`playerctl -p ytfast open https://music.youtube.com/watch?v=…`).

### GPU budget (2026-10-07)

How it was measured: `scripts/gpui-measure.sh` (intel_gpu_top render busy
for the whole desktop, app CPU in % of one core, 10 s), plus the render
time of each DRM client from `/proc/<pid>/fdinfo` (`drm-engine-render`:
GPUI's renderer, our effects device and KWin apart) and per-draw GPU
timings from Mesa (`INTEL_MEASURE=draw`, written to the app's stderr).
Profiling builds, test audio (`YTFAST_FAKE_STREAM`), Home with Quick picks,
1280x1000 window, light look, the machine shared with other agents' builds
(load 10-15). Songs opened over MPRIS, N for Now Playing, minimised with
KWin's "Window Minimize" shortcut.

Where the time went (before):

- **Window frames, not the effects.** Each window frame cost GPUI's
  renderer 8-12 ms of GPU time: the Intel UHD 630 runs at a low clock under
  this load, so every layer over the whole window costs about 1 ms. Home
  playing drew ~15 frames a second (GPUI 13.2%, effects device 2.8%, KWin
  1.1%); Now Playing 30 paced frames plus ~6 position ticks (GPUI 29.5%,
  effects 13.5%, KWin 11%). Effects off, the ticks alone cost GPUI 6.3%.
- In a frame: the kit root's background, the shell's base and the page
  panel (about 1 ms each); the ridge's GPUI paths (a full-window 4x MSAA
  clear and resolve per batch, two batches: 1.7 ms); in Now Playing the
  cover's `elevation::high` shadow (a 40 pt blur over the cover plus three
  blur radii: 2-3 ms) and the fallback gradient under the backdrop.
- Effects device: the backdrop shader 3-4 ms per frame at 528x448, the
  strip 1.35 ms, each readback copy 0.4-0.7 ms. Particles about 0.8 points
  of the desktop in Now Playing, the spectrum (GPUI quads) too little to
  measure. The upload of each frame into GPUI's atlas as a new image
  (`YTFAST_GPUI_VISUALS_SKIP=upload` kept the first one): no difference,
  so frames still go through new images (retired a frame later).
- KWin's 11% in Now Playing came from the unpaced tick frames between the
  paced ones; with only paced frames it is 1.3-2.9%.
- Tried and dropped: an integer hash instead of `sin` in the backdrop's
  noise made it slower on this GPU (7 ms instead of 4; 32-bit integer
  multiplies are emulated).

What changed: Now Playing at 20 frames a second with the backdrop in
every other one and the strip only when it would look different; the
bar's beat frames on the kick (bass at twice the step, at most 10 fps,
drift at 3 fps); the waveform painted by the effects layer, so ticks no
longer re-render the app in Now Playing; the cover shadow drawn by the
backdrop shader; the ridge drawn by the strip shader; the root's hidden
background cleared; no fallback gradient under a frame; per-frame colour
maths and far-away pixels out of the strip shader; the backdrop at 0.4 of
the panel; under reduced motion a tick wakes the layer only when the bar
changes.

GPU render busy for the whole desktop (points over the app closed) and app
CPU. `before` = main at 5956758, `after` = this branch; each pair back to
back, three after runs:

| State | GPU before | GPU after | CPU before | CPU after |
|---|---|---|---|---|
| app closed (baseline) | 8.7-9.2% | 8.3-9.0% | | |
| Home, paused | +0.2-0.3 | +0.0-0.6 | 0.0-0.1% | 0.0% |
| Home, playing | +16.9-19.0 | +7.2-8.2 | 6.2-7.6% | 4.1-4.2% |
| Now Playing, playing | +51.1-53.6 | +18.1-20.0 | 15.7-18.2% | 7.2-7.4% |
| minimised, playing | +0.1-0.3 | -0.1-+0.2 | 0.1-1.5% | 0.2% |
| reduced motion, Home playing | | +2.1 (was +6.0) | | 1.2% |
| reduced motion, Now Playing | | +2.0 | | 1.0% |

Per client after (one run): Home playing GPUI 5.8-6.0%, effects 1.3-1.4%,
KWin 0.9%; Now Playing GPUI 12.5-12.8%, effects 5.2-5.7%, KWin 2.5-2.8%.
Window frames: Home ~8 a second, Now Playing ~19. Home playing sits at the
+8 target (+7.2, +8.2, +7.5 over the closed baseline; +7.1-7.6 over the
paused app); the frame rate is what's left to trade.

Captures (`artifacts/gpui/`, gitignored): `gb0-sheet` (before), `fa3-sheet`
(after, Home and a Now Playing pair 1 s apart), `v1-bars` (bar pairs light
and dark), `v1d-np-sheet` (dark Now Playing pair), `shadow-cmp` (cover
shadow before/after), `ridge-cmp2` (ridge before, after light and dark),
`scale-cmp` (backdrop at 0.5 and 0.4, dark and light), `fr3-bars`
(reduced motion, the position moving).

### Shader warm-up (2026-10-07)

Before, the effects layer made the wgpu device (`Gpu::new`: Vulkan
instance, adapter, device, all blocking) in the frame of the first effect,
and each effect compiled its shader and pipeline in the frame it first drew.
Now:

- **Background device.** `Gpu` compiles all three pipelines (backdrop,
  strip, dissolve; `crates/visuals/src/pipelines.rs`) when it is made, so a
  renderer only makes buffers and textures (0.1-0.5 ms). The app makes the
  `Gpu` on GPUI's background executor (`src/visuals/device.rs`): once after
  the first visible frame (the warm-up, only with effects on), and again
  when an effect needs it after the layer dropped it for idling (30 s after
  the last frame, as before). The work starts in `on_next_frame`, so never
  inside a frame, and the layer takes the device when the task finishes;
  until then the effects don't draw and the views keep their plain
  backgrounds. Reduced motion, paused and minimised behave as before (they
  only decide what draws once the device is there).
- **Pipeline cache.** On Vulkan, when the adapter has
  `Features::PIPELINE_CACHE`, the pipelines go through a wgpu
  `PipelineCache` kept in `~/.cache/ytfast/gpu/`, one file per GPU and
  driver (`wgpu::util::pipeline_cache_key`, plus a hash of the driver name
  and version). wgpu checks the header and the driver's cache UUID and
  starts empty when they don't match. The file (about 141 KB here) is
  written to a temporary name and renamed, only when its length or header
  changed (the driver orders entries differently on every run), and other
  drivers' files for the same GPU are removed. Without the feature (the GL
  fallback) there is no cache; the device is still made in the background.
- **Per OS.** wgpu has pipeline caches only on Vulkan. Linux (Mesa) gets
  it. Windows uses the same Vulkan-or-GL device (DX12 is not asked for), so
  a Vulkan driver gets the cache, in the local app data cache directory
  under `gpu`. On macOS the device needs Vulkan (MoltenVK) or GL, so the
  cache would only apply through MoltenVK; Metal isn't used. Windows and
  macOS are untested on hardware; `cargo xwin check --target
  x86_64-pc-windows-msvc` passes.

How it was measured: profiling builds, test audio (`YTFAST_FAKE_STREAM`),
the 1280x1000 window, a song restored in the player bar at start (so the
strip draws in the first frames), then a song opened over MPRIS (first play:
the bar cover's dissolve), N (first Now Playing: the backdrop), MPRIS Next
(first track change inside Now Playing: its dissolve).
`YTFAST_GPUI_FRAME_LOG=12` logs frames whose UI thread work (shell render
through the last paint) took over 12 ms, and every frame that set up an
effect with the time that took; `visuals:` lines time the instance,
adapter, device, each shader and pipeline. Three runs per condition:
**warm** (all caches warm), **cold** (`drop_caches` first) and **no driver
cache** (cold plus `MESA_SHADER_CACHE_DISABLE=true`, as after a driver
update; after the change our pipeline cache is still there). The machine was
shared with other agents' builds (load 10-13), so frames of 15-50 ms happen
with the effects off too.

Effect set-up on the UI thread / the frame it happened in, ms (ranges over
the runs):

| Event | before warm | before cold | before no driver cache | after (all) |
|---|---|---|---|---|
| start: device | 71-103 / 116-137 | 86-105 / 128-150 | 76-126 / 122-219 | 0 |
| start: strip | 4.5-18 / 15-32 | 5.7-13 / 19-31 | 87-122 / 103-143 | 0.2-0.5 / 11-28 |
| first play (dissolve) | 1.3-3.7 / 7-13 | 2.4-4.1 / 10-28 | 17-37 / 23-44 | 0.0-0.1 / 4-10 |
| first Now Playing (backdrop) | 3.8-12 / 12-24 | 4.4-12 / 12-23 | 136-158 / 145-166 | 0.2-0.3 / 8-55 |
| first track change (dissolve) | 1.2-1.4 / 20-24 | 1.2-1.8 / 20-27 | 3.4 / 23-29 | 0.1 / 14-27 |

Where the device time went before: instance 44-95 ms (loading the Vulkan
drivers), adapter 15-22, device 7-20. Pipelines with Mesa's disk cache warm
took 0.4-15 ms each, without it backdrop 133-155, strip 84-119, dissolve
16-36.

After, in the background: the device ready 93-272 ms after the first frame
warm, 147-238 cold, 108-143 without the driver cache (all pipelines from our
cache in 8-10 ms), and 384 ms on a first run with no cache at all
(pipelines 234 ms). The frames during that time were 16-51 ms at most, as
at start-up with the effects off (15-110 ms). After the change no frame
spent more than 0.5 ms on effect set-up; the frames over 16 ms that remain
in the table (Now Playing opening, a track change re-rendering the app) did
no set-up and match runs with the effects off (`YTFAST_GPUI_VISUALS=0`:
the longest frame 19-26 ms on play, 14-49 ms opening Now Playing, up to 21 ms
on Next). The device dropped after 30 s idle was made again in 73 ms on the
next play, without a set-up frame. The first strip frame after start still
costs its still-picture wait (`frame_now`, 8-13 ms like every still strip
frame), which is not set-up.

Captures (`artifacts/gpui/`, gitignored): `trial-sheet` and
`after-trial-sheet` (first play, Now Playing, after Next; before and
after), `warmup-bar-cmp` (the strip before and after), `warmup-np-cmp`
(Now Playing before and after): the effects look the same.

## Now Playing (M8)

Code: the crate `gpui/crates/visuals` (`ytfast-visuals`, no GPUI) and the
app side in `gpui/src/visuals/`.

- **Crate**: `Renderer` (on the shared `Gpu`, `shaders/backdrop.wgsl`,
  offscreen target and readback; `frame(params)` returns BGRA bytes), `Cover` (48x48
  pre-blurred upload and a four-colour palette), `AudioTap` (PipeWire tap,
  FFT, 32 bands plus bass, kick and level) and `waveform` (ffmpeg decode of
  the URL mpv plays, 400 values, cached per video id in the cache
  directory). `just check visuals` and `just test visuals` check and test
  it, `just shaders` validates the WGSL with naga.
- **Layers** (`visuals::shell`, what `MusicApp` renders): `Effects` under
  the app (backdrop over the page panel and the spectrum strip), `Content`
  (the app's views, an `AnyView::cached` entity while Now Playing shows) and
  `Flight` over it (the cover flying between the player bar and Now
  Playing, `motion::SLOW`, ease-out). `Effects` notifies itself from a
  20 fps timer (the backdrop renders in every other frame); the notify marks `MusicApp` dirty too, but its render is
  only the shell, the app's views come from the cache. Mouse and key input
  drop the cache for one frame (a slider drag changes a model, not a view).
- **Backdrop**: the cover blurred, domain-warped and turning slowly over a
  gradient of its palette, a soft bloom of its brightest colours, motes
  (particles) drifting up and flaring on the beat, a bass pulse, dither.
  New covers cross-fade over 1.2 s. Tone-mapped per theme so text keeps
  4.5:1: dark look capped at luminance 0.045 (measured 0.022-0.042 on a red
  cover), light look pressed into 0.645-0.8 (measured 0.64).
- **Cover shadow**: the theme's `elevation::high` under the large cover,
  drawn by the backdrop shader (two closed-form blurred boxes) while the
  backdrop shows; the cover leaves its own out then (`paints_cover_shadow`).
- **Spectrum**: 64 bars mirrored around the centre (lows in the middle)
  above the title, painted with GPUI quads in `text` at 28-88% opacity.
- **Waveform**: under the title, thin bars, the played part in the accent
  colour; a click seeks. A faint line until the decode (1.1-1.3 s) is done.
  The effects layer paints it in `Slot::Waveform` (the view keeps the
  click), so a position tick doesn't re-render the app's views.
- **Stopping**: no frames and no tap when Now Playing is closed, the window
  is hidden (minimised), playback is paused or motion is reduced (then one
  still frame per cover, no spectrum, no particles, no flight). The renderer
  (the second Vulkan device) is dropped 30 s after Now Playing closes.
  Reduced motion is `theme::reduced_motion` (the desktop portal) or
  `YTFAST_GPUI_REDUCED_MOTION=1`.
- **Settings** (env): `YTFAST_GPUI_VISUALS=0` off, `YTFAST_GPUI_VISUALS_FPS`
  (20), `YTFAST_GPUI_VISUALS_UNCACHED=1` (no cached app view, to measure
  it), `YTFAST_GPUI_VISUALS_SKIP=backdrop,strip,spectrum,particles,upload`
  (leave one out, to measure it), `YTFAST_GPUI_VISUALS_FLIGHT_MS` (slow the
  flight down to look at it).

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

## Taste (M21, 2026-10-07)

The rules every effect, preset and capture is checked against.

- **Restraint.** One focal effect at a time; ambient effects stay under the
  content and never compete with covers, text or controls. If you notice
  an effect before the music, it is too strong. Default should look calm
  and expensive (think Apple Music's animated backgrounds); Vivid is
  opt-in.
- **Motion.** Ambient motion runs on time scales of seconds, eased, never
  linear ramps or hard on/off, and fluid at the chosen frame rate (no
  stepping: the backdrop draws with every paced frame). The music moves
  things through smoothed envelopes: no strobing, no element pulsing on
  every kick, no flashes of brightness. Reduced motion holds a still frame.
- **Colour.** From the cover's palette in OKLab, moderate chroma, no neon,
  no pure white or black blooms; text keeps 4.5:1 in light and dark;
  gradients are dithered so nothing bands.
- **Shape.** Soft falloffs and no visible geometry in ambient effects
  (frames fade out before their edges); the visualiser may be crisp but
  sits on the layout (baselines, margins) and uses the app's radii.
- **Checks.** Captures in light and dark with a vivid, a muted and a
  near-monochrome cover, viewed at full size, until nothing looks cheap or
  busy.

## Settings → Visuals and the visualiser (M21, 2026-10-07)

**Settings.** `visuals::config` keeps every effect's settings in
`visuals.json` in the config directory (versioned; missing and unknown
fields load; ranges are clamped on load). Presets: Off (`on: false`, every
effect at once), Calm, Default (the backdrop as before M21, the player
bar's glow at 55% and its halos at 50%: they read as too strong)
and Vivid (glow and halos as before, a livelier backdrop and more
sparkles). A preset keeps the visualiser's choices, Stage's, the
particles' look (size, softness, depth, direction, colour), the wave's
shape and the frame rate. `YTFAST_GPUI_VISUALS=0`, `_PRESET`, `_FPS`,
`_FLIGHT_MS`, `_SKIP` and `YTFAST_GPUI_VISUALIZER` still override without
saving. Settings → Visuals is a view of its own in tabs (General, Backdrop,
Particles, Player bar, Visualiser, Transitions; `views/settings/visuals`),
a card per effect, Reset per tab; Ctrl+K "visuals" offers the four presets.
Sliders show their change in the next frame and save when let go.

**Frame rate.** 15, 20, 30, 60, 120 or the display's (frames then come
with GPUI's `request_animation_frame`). Default: the display's on macOS,
30 on Windows, 20 on Linux, where a window frame costs the UHD 630 6-10 ms
and 30 doubles Now Playing's GPU use (table below). The backdrop draws with
every other window frame, 10 to 30 a second: it is blurred and moves
slowly. What stepped before (on a 120 Hz Mac: the motes at 10
frames a second) now moves with every window frame.

**Sparkles and the wave.** After the PS3's XrossMediaBar. The light wave is
one to three translucent ribbons of the palette in `backdrop.wgsl` (soft
enough for its 0.4 scale), on a clock of the swirl speed times the wave's
speed. The sparkles are GPUI quads over the backdrop (`visuals::ambient`):
one per grid cell in three depths (far ones smaller, fainter, slower, as
far as the depth spread says), from an integer hash of the cell, so they
don't shimmer while the grid drifts; radius in device pixels (0.7 to 1.4
by default), a halo quad only for the near and large ones, a twinkle over
seconds, brighter near the wave, and lifted at most half the reaction by
the music's smoothed level. First tried as a full-size frame of the
visualiser shader: 7% CPU and 12% GPU at 30 fps here, mostly the read-back
and upload, so they became quads (about 300 at amount 1; each costs GPUI
about 2 µs of CPU a frame, which is why there aren't more).

**Visualiser.** `ytfast_visuals::Visualizer` (`visualizer.wgsl`, compiled
with the other pipelines, so warm-up and the pipeline cache cover it) draws
bars, mirrored bars, a ring round the cover, a line or a particle field
from `Bars` (bar count, sensitivity, rise smoothing, fall speed, frequency
range and spacing, peak caps), and since M22 an oscilloscope from the
samples (below), straight alpha, painted over the backdrop. Regions: Now Playing's strip over the spectrum (or in its place); the ring
round the cover, which shrinks the cover to leave the ring its room inside
the panel (`now_playing_ring_room`, and `stage_ring_room` beside Stage's
lyrics); a band along the bottom of Stage's body that the body keeps free
(`stage_band`); the whole scene for particles. It draws only with paced
frames while music plays and the window is visible, not under reduced
motion. The ring renders at full size on whole pixels: at 0.75 its frame's
edge left a faint line in Stage.

**Colour and calm.** A sepia or black-and-white cover no longer turns the
window muddy olive: the backdrop's tone mapping keeps less of the cover's
colour the less colourful its palette is (from 25% under OKLab chroma
0.05 to all of it at 0.12), and the visualiser's stops cap their chroma
the same way, so such covers give a quiet grey with a trace of their hue.
The stops run from deeper to lighter along the spectrum, so bars carry a
gradient even from a one-colour palette. Bars and mirrored bars are pills
with gaps that deepen from base to tip, with a gentler glow and fainter
peak caps; levels are drawn on a calmer scale (0.86 of v^1.25) so loud
passages don't pin the top; in Stage and the full window the bands keep a
7% margin at each side. Now Playing shows one music graphic, the
spectrum or the visualiser, never both stacked (the earlier "Both" loads
as the visualiser). Captures `h-sepia*`, `h-bw`, `h-vivid*`.

**Motion.** The cover flight, the cover dissolve and the audition ring take
Settings → Motion's speed and reduced motion (`motion::duration`); the
full-window visualiser fades in through `with_motion`.

**Cost** (`scripts/gpui-measure.sh`, profiling builds, test audio, signed
out, 1280x1000, Now Playing after N; `main` = 142d187, back to back, app
closed 8.9%):

| State | GPU main | GPU M21 | CPU main | CPU M21 |
|---|---|---|---|---|
| Home playing | 15.9% | 15.3-15.5% | 4.6% | 4.8-4.9% |
| Now Playing, default (20 fps) | 26.9-27.2% | 26.4-27.3% | 7.6-7.7% | 8.9-9.0% |
| Now Playing, 30 fps* | | 50.9% | | 13.7% |
| Now Playing, 60 fps* | | 63.8% | | 23.0% |

\* Before the backdrop went to every other frame. The default costs the
GPU what main did; the sparkles add about 1.3 points of one core.

Captures (`artifacts/gpui/`, gitignored): `v-np-*` and `v-full-*` (each
style in Now Playing and the full window), `v-stage-mirrored`,
`v-stage-ring`, `v-max-*`/`v-min-*` (extremes, light and dark), `x-l-*`
and `f-dark-*` (the tabs, light and dark), `y-*` (three covers in light
and dark), `z-*-full` and `c3` (sparkles at full size), `f-np-vivid`,
`f-np-off`.

## The oscilloscope (M22, 2026-10-07)

**Samples.** The engine's tap keeps both channels now: the output
callback stores each frame's left and right in one 64-bit atomic (still
one store a frame), and readers take mono or stereo. The spectrum thread
reads stereo, analyses the mix and, while the scope asks
(`AudioTap::recent`, within the last second), keeps the newest 8192
frames; the scope copies the newest 72 ms of them each paced frame.

**Trigger** (`crates/visuals/src/scope.rs`). A trace is 30 ms of sound.
It starts on a rising zero crossing of a copy low-passed at 400 Hz (so
the fundamental triggers, not the hiss on top), armed only after the
signal fell below 8% of the recent peak, with the low-pass's lag taken
off. Of the crossings in the last 40 ms, the one whose window is most
like the trace on screen wins (64 probes, older ones costing a little),
so a steady note stands still instead of hopping between periods; then
each frame eases a quarter of the way back to the last (at 20 fps). An
automatic gain follows the loudest sample (up within a few frames, down
over about a second) times the sensitivity, with a tanh limit, so quiet
songs and low volume still fill the band and loud passages don't hit
its edges. A test feeds a tone at five phases and checks the traces
match.

**Channels.** Mono (the mix), Stereo (left over right, each in half the
band) and X/Y (a goniometer: mid up, side across, so mono music is a
vertical line and width spreads it sideways; the newest 20 ms, the older
part fainter, not eased since two figures averaged are neither). The test
audio is dual mono (its side channel at -91 dB), so its X/Y is a line;
`sc4` used a synthetic stereo file to show the figure.

**Drawing.** A style of the visualiser pipeline (style 5), so warm-up and
the pipeline cache cover it. The traces go in the uniforms as up to 1024
values packed four to a vec4, plus the X/Y figure's 16 runs of 16
segments with their bounds. Strokes are distances to line segments:
anti-aliased at any slope, the glow under the body. A trace pixel only
measures the segments within reach of its column, and skips them when it
is above or below all of them; an X/Y pixel only the runs whose bounds it
is near. Colour runs along the band (the X/Y figure along its age) from
the same stops as the bars; opacity, glow and colours are shared with
the other styles. Thickness (1 to 6 points, default 2.5) is a new slider
for the line and the scope; the line keeps its 2.5. The scope uses Now
Playing's strip (centred, 8 points round it) and the same bottom band as
the bars in Stage and the full window, with 15% of the band (4 to 32
points) kept free at the top and bottom and the trace fading out through
the side margins. Settings show Channels and Sensitivity for the scope
instead of the bars' rows; `YTFAST_GPUI_SCOPE=mono|stereo|xy` overrides.

**Cost** (`scripts/gpui-measure.sh`, profiling build, test audio, signed
out, dark, 1280x1000, the visualiser in Now Playing and the full window
(V), two rounds run back to back with Bars, app closed 8.9-9.0%, load
3-5):

| State | GPU Bars | GPU Scope mono | stereo | X/Y | CPU Bars | CPU Scope |
|---|---|---|---|---|---|---|
| Now Playing | 29.2-30.0% | 29.9-30.2% | 30.0% | 29.6-30.2% | 9.7-9.8% | 9.5-9.8% |
| Full window | 29.4-29.9% | 28.7-29.1% | 31.4-32.1% | 30.1-30.7% | 8.6-8.7% | 8.5-9.0% |

Mono and X/Y cost what Bars do; left over right in the full window about
2 points more (two traces over the whole band; before the culling about
6). The scope's own CPU (copying 72 ms of frames, the trigger search)
doesn't show against the frame's.

Captures (`artifacts/gpui/`, gitignored): `sc1-*` (dark, vivid cover,
mono: Now Playing, full window), `sc2-*` (light, muted cover, stereo: Now
Playing, Stage, full window), `sc3-*` (dark, X/Y, dual-mono audio),
`sc4-*` (X/Y with a synthetic stereo file), `sc5-*` (three traces a
second apart), `sd-*` and `sl-*` (dark muted and light vivid, mono, all
three places), `sc9-tab` (Settings), `m5*` (the measured states).

# The spike (2026-10-06)

Measured on 2026-10-06: Fedora 43, KDE Plasma 6 Wayland,
Intel UHD 630 (Vulkan), 1920x1080 at **120 Hz**, gpui-kit 0.7.1 / gpui-pre
0.3.8 / wgpu 29.0.4, release build. The machine was shared with other agents'
builds (load 3 to 8 for the numbers below, 20+ for the first runs), so CPU
figures are ±3 points. The spike ran with `YTFAST_GPUI_VISUALS_SPIKE=1`
(removed since) and drew everything into the page area; its file names
below (`gpu.rs`, `spike.rs`) are now `renderer.rs` and `effects.rs`.

## The 3D scenes (M30, 2026-10-07)

From the browser spike (docs/gpui/SPIKE-3D.md), the three the maintainer kept:
XMB (the PS3's wave, ported from linkev/PlayStation-3-XMB, MIT, its notice
in the shader), Ridges (a flight over land whose ridges are the spectrum)
and Aurora (curtains of the cover's colours over a lake).

Code: `crates/visuals/src/scene.rs` (`Scene`, `Pace`, `seed`) and
`shaders/scene_*.wgsl`; app side `src/visuals/visualizer.rs`
(`update_scene`), the visualiser's styles in `config.rs`, Settings →
Visuals → 3D (`views/settings/visuals/cards.rs`, `Card::Scenes`).

- **Styles of the visualiser.** XMB, Ridges and Aurora are styles beside
  the bars; they fill the whole scene (Stage, the full window) or Now
  Playing's panel (with its corners), opaque, where the visualiser shows.
  Behind text (Now Playing, Stage) they are toned like the backdrop
  (`tone_dark`/`tone_light`, so text keeps 4.5:1); in the full window at
  full strength. While one shows, the backdrop and its sparkles rest (no
  frames, nothing painted under it) and the views draw the cover's shadow.
- **Shaders.** `scene_common.wgsl` (the spike's parameters, 19 vec4s,
  noise and tone mapping) goes in front of each scene in one module. The
  spike's integer hashes became Dave Hoskins' float hashes (integer
  multiplies are slow on the UHD 630, above); the scenes are otherwise the
  spike's. XMB is three passes (the gradient, a 100x100 grid mesh blended
  over it with a fresnel alpha, 2000 instanced sparkles added); Ridges and
  Aurora are one raymarched pass each. The pipelines compile with the
  others when the device is made (about 150 ms more on Metal, which has no
  pipeline cache; in the background).
- **Music.** `Pace` keeps slow envelopes (bass, mids, highs, level) and
  two fast ones (energy and highs: 0.08 s up, 0.7 s down), and clocks
  whose speed follows them (integrated, never time times an envelope).
  XMB's wave flows 0.6x to ~2.8x with the energy and its sparkles with the
  highs; its shape never changes with the music. Settings → Visuals → 3D's
  Music reaction sets how much a scene behind text follows (0.5 as
  designed); the full window follows all of it.
- **Resolution.** XMB renders at full size (its lines and sparkles are a
  pixel or two), Ridges at 0.6 and Aurora at 0.5 of device pixels, times
  the Resolution setting, at most full size and 2048 wide.
- **Settings → Visuals → 3D.** The scene (it is the visualiser's style),
  where it fills (Now Playing, Stage), Strength, Detail (the raymarch's
  steps), Resolution and Music reaction. Presets keep them.

Measured 2026-10-07, Apple M5 Pro, profiling build, test audio
(`YTFAST_FAKE_STREAM`), the full-window visualiser in a 1280x852 window at
2x, 120 frames a second, `macmon` (no sudo) over 8 s per state, back to
back:

| State | Total | CPU | GPU | GPU busy | Scene's frame |
|---|---|---|---|---|---|
| Home, playing (visualiser closed) | 0.98 W | 0.85 W | 0.13 W | 3% | |
| Bars | 1.82 W | 1.25 W | 0.57 W | 10% | |
| Particles | 1.83 W | 1.19 W | 0.64 W | 8% | |
| XMB, 2048x1312 | 2.39 W | 1.86 W | 0.53 W | 7% | submit 0.09 ms, copy 0.25 ms |
| Ridges, 1536x984 | 3.30 W | 1.38 W | 1.92 W | 20% | submit 0.15 ms, copy 0.20 ms |
| Aurora, 1280x820 | 2.80 W | 1.24 W | 1.56 W | 16% | submit 0.16 ms, copy 0.16 ms |

XMB costs the GPU less than the bars over the backdrop; its extra is CPU,
the upload of a 2048x1312 frame 120 times a second (the readback path; the
zero-copy question in SPIKE-3D.md is what would remove it). Ridges and
Aurora cost the GPU 1-1.5 W more than the bars at 120 fps. Tried and
dropped: Ridges' spectrum transform out of the pixel shader (no change,
1.49 ms either way: the march is the cost). The levers are the frame rate,
Resolution and Detail.

`cargo run -p ytfast-visuals --release --example scene_bench` times each
scene with its readback at the sizes the app uses (one window at 2x), to
run on the UHD 630 and a MacBook with the app closed. On the M5 Pro:
XMB 0.74 ms a frame (2560x1600) and 0.58 ms (Now Playing's panel), Ridges
1.35 / 0.89 ms, Aurora 1.06 / 0.88 ms. Not measured yet: the UHD 630
(Linux) and Windows.

Now Playing with a scene (captured on the Mac, dark and light; the scenes
run at Now Playing's pace, about 90 frames a second there): the cover,
title, waveform and Up next read over all three in both looks. In the
light look the scenes nearly vanish (XMB a faint wave, Ridges and Aurora a
pale haze): `tone_light` presses them into luminance 0.645-0.8. Keeping
them there needs a scrim behind the text instead of the whole-panel
squeeze (SPIKE-3D.md).

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

**Since M22 (2026-10-07): the audio engine's own tap, in process, on every
OS.** mpv and the PipeWire tap below are gone (M23). The output callback of
`ytfast-audio` copies its mix (after the equalizer and the decks' volumes)
as mono into a lock-free ring of atomics (`audio/src/tap.rs`) while an
`AudioTap` is open, and skips the copy otherwise; the spectrum thread takes
1/60 s of it per frame and drops anything more than 0.2 s behind. The
callback hands over ~43 ms at a time, so the bands trail the device by
about one period. What follows is the history.

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

**Since M23 (2026-10-07): decoded with the audio engine's own decoder**
(`ytfast_audio::decode_mono`: range requests, symphonia and libopus, to
~8 kHz mono by block averages), from the URL the resolver cached, in the
background and cached per video id. No ffmpeg, no mpv IPC. History below.

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
`spectrum.rs` (the engine's tap, FFT and bands), `waveform.rs` (the
engine's decoder, cache), `audio/src/tap.rs` (the ring); app side `gpui/src/visuals/mod.rs`
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
- Every `.wgsl` validates with naga (`just shaders`).
