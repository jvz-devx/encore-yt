# GPUI notes

What we learned about GPUI and gpui-kit 0.7.1 building this app: API
patterns that aren't obvious from the sources, pitfalls we hit, how the
headless UI tests work, and the media controls on Windows and macOS. The
code in `crates/app` is the reference for everything else; the kit's
sources are in Cargo's registry (gpui-component, gpui-base, gpui-pre).

## Dependencies and setup

- `gpui-kit 0.7.1` pins `gpui-pre =0.3.8` and `gpui-pre-platform =0.3.8`
  (wayland, x11, font-kit). Don't list gpui or the platform crate yourself;
  move the kit and `gpui-pre-reqwest-client` together.
- `use gpui_kit::*;` is GPUI; `gpui_kit::component` is gpui-component,
  `::base` and `::assets` the rest. `extern crate gpui_kit as gpui;` in
  `main.rs` lets the derive macros (`IntoElement`, `Action`) find `gpui`.
- `img("https://…")` needs `cx.set_http_client(…)`, or remote URLs go
  straight to the fallback. `.image_cache(retain_all(…))` on an ancestor
  keeps decoded images across frames.
- The kit's icon bundle (`gpui_kit::assets::Assets`) returns `Err` on an
  unknown path: chain it last behind the app's own asset source, or icons
  go missing.

## Backend thread to UI

The backend stays GPUI-free. It sends events on a std channel and wakes the
UI through a `smol::channel::bounded(1)` with `try_send`, so wakes
coalesce; a foreground task drains every waiting event on each wake and
notifies once.

- Foreground tasks: `cx.spawn(async move |this, cx| …)` returns a `Task`;
  keep it (dropping cancels) or `.detach()`. `this.update(…)` fails once
  the entity is gone.
- Background work: `cx.background_spawn(…)`; timers with
  `cx.background_executor().timer(d)`.

## Pitfalls

- `h_flex()` sets `items_center`, so a `v_flex` child collapses and is
  centred. Give the column `.h_full()` (and the row `.min_h_0()`) so
  `flex_1` lists get space. `uniform_list` rows size to their content:
  add `.w_full()`.
- On Linux GPUI quits when the last window closes; the app sets
  `QuitMode::Explicit` to live on in the tray.
- An entity can outlive its window and be shown by a new one. gpui-pre
  tracks each entity's current window (the last that rendered it), and
  `subscribe_in` and `cx.with_window(entity_id, …)` follow it;
  `spawn_in(window)` and `observe_*`/`on_blur` with a window stay bound to
  the first window and stop when it closes.
- A monitor in DPMS off sends no frame callbacks, so nothing redraws
  (`kscreen-doctor --dpms on`). From a non-session shell, export
  `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR` and `DBUS_SESSION_BUS_ADDRESS`.
- KDE finds the window's icon and name through the desktop file named
  after the Wayland app id (`io.github.jvz-devx.encore-yt.desktop`);
  without one installed it shows a generic icon.

## Keyboard focus

- A div's `tab_index`/`tab_stop` apply only to the handle it makes itself.
  With `track_focus(&handle)` the handle's own flags count: make it with
  `cx.focus_handle().tab_stop(true)`, or Tab skips it.
- When the focused element stops being drawn, keys dispatch from the
  window's root and every context binding goes dead. `desktop::keep_focus`
  (`cx.on_focus_lost`) moves the focus to `focus_lost_restore_target`.
  Changing reduced motion rebuilds the tree under Settings, so that
  happens there too.
- A focusable element takes the focus on mouse down and calls
  `prevent_default`, which stops the `active` style of elements under it
  that run later; make the row or card itself focusable, not a layer on it.
- `.hover()` twice on one element panics in debug builds ("hover style
  already set").
- Enter and Space "click" a focused element on the key's release; GPUI's
  `simulate_keystrokes` sends only presses (`Ui::press` sends both).
- A drop shadow is painted under the element's own fill, so a ring made of
  one fills a transparent or translucent element. Use an inset shadow, a
  border, or a line drawn over it.
- A RenderOnce component gets the `Window`: `views::page::item_keys`
  uses one to keep a keyed focus handle and draw the ring only while
  `is_focused && last_input_was_keyboard`.

## UI tests (headless, `just test app`)

`crates/app/src/ui_tests/` runs the real `MusicApp` in a window of GPUI's
test platform (`#[gpui_kit::test]`, the kit's `test-support` feature as a
dev-dependency).

- Seam: `MusicApp::with_link(Link::Fake { .. })` (`crates/app/src/link.rs`,
  test builds only) instead of `Backend::start`. A test pushes `Event`s
  through a std channel and drains them by hand (`MusicApp::drain`); the
  app's `Command`s (and the desktop remote's) land on a tokio channel the
  test reads. The desktop services (instance socket, tray, MPRIS, signals)
  don't start, the theme skips the portal (`theme::init_without_desktop`),
  effects are off (`visuals::enabled` is false under `cfg(test)`), and
  `Paths` point at directories that are never made.
- Finding elements: views name them with `.debug_selector(|| ..)`, recorded
  only in debug builds with gpui's `test-support` and a no-op otherwise;
  `VisualTestContext::debug_bounds(name)` gives their bounds in the last
  frame. Names carry the text a test checks ("bar-title:{title}",
  "play-button:pause", "menu-entry:Play next", "suggestion:{text}").
- Input: `simulate_click`, `simulate_mouse_down/up` (right-click),
  `simulate_keystrokes("ctrl-,")`, `simulate_input("text")`; timers with
  `executor().advance_clock(d)` (the search debounce). Draw with the kit's
  `TestWindowExt::render_frame` after each step.
- Test windows start inactive and GPUI sends focus events only to the
  active window, so an `InputState` never emits `InputEvent::Focus` until
  `window.activate_window()`.
- What the test platform can't do: no text shaping (`NoopTextSystem`), so
  rendered text, truncation and line widths can't be checked, only a name a
  view gave an element; no pixels or colours; no GPU, so the effects layer,
  the waveform and the spectrum are untested; no images (the asset source
  and HTTP client are stubs, so covers and icons show their fallbacks); no
  D-Bus (portal light/dark, MPRIS, tray, notifications); no real backend
  (audio engine, resolver, InnerTube). The kit's `find`/`click` helpers
  need `.test_support()` on each element, which changes its type in test
  builds; `debug_selector` doesn't.
- The first test build compiles gpui with `test-support` (many minutes);
  after that a change rebuilds in seconds and the tests run in under a
  second, in parallel.

## Media keys on Windows and macOS (M15)

`crates/app/src/desktop/media.rs`, through the `souvlaki` crate (no default
features, so no D-Bus on Linux, where the backend's MPRIS stays). It follows
the same `desktop::Now` watch as MPRIS and drives the app through the same
`Remote`; the controls live while the main window is open (closing it quits
outside Linux).

- Windows: System Media Transport Controls need the window's HWND, from
  GPUI's `Window` via `raw-window-handle` 0.6 (the version gpui-pre uses).
  The overlay doesn't advance the timeline itself, so the position is
  republished every 5 s while playing, and at once on a seek or pause.
- macOS: `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter` belong to the
  process; souvlaki calls them from GPUI's foreground (the main thread).
  The cover is fetched by the system from YouTube's thumbnail URL.
- Checked here with `cargo xwin check --target x86_64-pc-windows-msvc` and
  by CI builds for macOS. Untested on real hardware: the Windows overlay,
  lock screen and keyboard media keys, macOS Control Center's Now Playing,
  the media keys and AirPods controls.
