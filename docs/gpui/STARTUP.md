# Startup time (M17)

Measured 2026-10-07 on the dev box (Intel UHD 630, 6 cores, KDE Plasma 6 on
Wayland), `just profiling` builds, signed in, with a restored queue
and `YTFAST_FAKE_STREAM` set. Other agents were building at the same
time (load average 4 to 12), so the before and after binaries ran in
alternation and the tables give medians of six launches each.

## How to measure

Every launch logs its milestones, timed from the top of `main` (here a
warm launch after the changes):

```bash
grep 'startup:' ~/.cache/ytfast/ytfast-gpui.log
```

```text
startup: main at unix 1791368271364 ms
startup: logging ready at 0 ms
startup: platform ready at 26 ms
startup: fonts and theme ready at 27 ms
startup: backend started at 129 ms
startup: app state built at 129 ms
startup: first frame drawn at 129 ms
startup: window open at 199 ms
startup: first frame presented at 199 ms
startup: Home from cache presented at 199 ms
startup: account checked at 327 ms
startup: Home fresh presented at 993 ms
```

"Presented" lines come from the frame after the one that drew the state, so
the state was on screen by then. The `main at unix` line gives the wall
clock, to time the exec from a launcher's own timestamp (6 ms warm, about
40 ms cold). A cold start is the first launch after
`sync; echo 3 | sudo tee /proc/sys/vm/drop_caches`; a warm one follows
another launch.

## Before and after

Milliseconds from `main`, medians of six launches.

| Milestone | Warm before | Warm after | Cold before | Cold after |
|---|---|---|---|---|
| Platform ready | 38 | 34 | 480 | 540 |
| Fonts and theme ready | 51 | 38 | 501 | 546 |
| Backend started (joined, after) | 217 | 146 | 1162 | 1148 |
| First frame presented | 317 | 215 | 1292 | 1350 |
| Home from cache presented | 401 | 215 | 1401 | 1350 |
| Account checked | 521 | 331 | 1478 | 1163 |
| Home fresh presented | 1348 | 1239 | 2192 | 1787 |

Warm, the window now shows Home from its saved copy in the first frame,
at about 0.2 s (it was an empty Home at 0.3 s, then the saved Home at
0.4 s). Cold, the content arrives about as soon as before, and the account
check and the fresh Home come 300 to 400 ms earlier. The cold first frame is
no faster: disk reads dominate it, and the threads that now start early
read from disk at the same time as the platform.

## What changed

- **The effects' GPU device is made on a thread.** With a song restored,
  the effects asked for their wgpu device in the first frame's render;
  making it (Vulkan instance, adapter and device) took 80 ms or more on the
  UI thread. The first frame now paints the plain bar, and the effects take
  over once the device is there (`crates/app/src/visuals/device.rs`).
- **The portal and the cover art client start on threads.** Reading the
  desktop's colour scheme is a D-Bus round trip (capped at 300 ms when the
  portal is slow), and the HTTP client loads the system's root
  certificates. `main` starts both right after logging; the platform's
  start overlaps them.
- **The backend starts before the platform.** It used to start inside the
  window's build closure, so restoring the session, reading the cookies,
  checking the account and loading Home waited for the platform and the
  window. `main` now starts it on a thread, which asks for Home at once
  (`app::Early`). The window's first drain finds Home's saved copy, so the
  first frame shows it.

## Where the rest goes (warm, after)

- About 30 ms: GPUI's application and the Wayland connection.
- 100 to 150 ms: opening the window. GPUI's wgpu context enumerates every
  adapter (Vulkan and GL, so EGL starts too), makes the device and its
  pipelines. This is upstream code; the app can't move it.
- 60 to 110 ms: drawing the first frame with Home in it: layout, shaping
  and rasterizing the glyphs of every visible shelf. An empty first frame
  drew in half that, but showed nothing.
- Home's fresh copy is a network round trip (InnerTube `browse`), about a
  second after the request.
