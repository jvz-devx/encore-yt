# GPUI spike notes

Standalone package (own `[workspace]`), run on Fedora 43 KDE Wayland, UHD 630, wgpu 29 (Vulkan).

## Dependencies

```toml
[dependencies]
gpui-kit = "=0.7.1"   # facade: GPUI + platform (wayland, x11, font-kit) + base + component + assets
reqwest_client = { package = "gpui-pre-reqwest-client", version = "=0.3.8" }  # for img("https://...")
smol = "2"            # async channel for the backend bridge

[profile.dev.package."*"]
opt-level = 2
```

- `gpui-kit 0.7.1` pins `gpui-pre =0.3.8` and `gpui-pre-platform =0.3.8` with
  `font-kit,x11,wayland,runtime_shaders`. You do **not** list gpui or the platform crate yourself.
- `use gpui_kit::*;` is GPUI; `gpui_kit::component` (= gpui-component 0.7.1), `::base`, `::assets`.
- The reqwest client crate is not re-exported by the kit; keep its version equal to gpui-pre.
- Before using `#[derive(IntoElement)]` here, add `extern crate gpui_kit as gpui;` (per kit docs).

## Init

```rust
gpui_kit::application().with_assets(AppAssets).run(|cx| {
    gpui_kit::init(cx);                          // component + base + theme; call first
    Theme::change(ThemeMode::Dark, None, cx);
    let http = reqwest_client::ReqwestClient::user_agent("ytfast-gpui/0.1")?;
    cx.set_http_client(Arc::new(http));
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some("Music".into()), ..Default::default() }),
        window_bounds: Some(WindowBounds::centered(size(px(1100.), px(720.)), cx)),
        app_id: Some("ytfast-gpui".into()),
        ..Default::default()
    };
    // Wraps the view in gpui_base::Root (dialogs, notifications, focus).
    gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| MusicApp::new(window, cx)))?;
    cx.on_window_closed(|cx, _| cx.quit()).detach();
});
```

Theme: `ActiveTheme` gives `cx.theme().background/.border/.accent/.sidebar/.radius`.
`font_bold()`/`font_semibold()` need `gpui_kit::component::StyledExt` in scope.

## Icons

Default bundle (`gpui_kit::assets::Assets`) holds ~100 icons (search, play, pause, heart, ...),
not house/compass/library/skip-*. Add more with the kit macro and chain the sources:

```rust
gpui_kit::assets::icon_assets!(ExtraIcons, [House, Compass, LibraryBig, SkipBack, SkipForward, Volume2]);
// AssetSource::load: ExtraIcons first (Ok(None) on miss), then Assets (Err on miss).
// gpui_kit::assets::IconName::House works wherever impl Into<Icon> is taken.
// gpui_kit::assets::AllAssets embeds all of Lucide instead (bigger binary).
```

## Sidebar

```rust
Sidebar::new("nav").w(px(220.))
    .header(SidebarHeader::new().child(div().font_bold().child("Music")))
    .child(SidebarGroup::new("Browse").child(SidebarMenu::new().children([
        SidebarMenuItem::new("Home").icon(Lucide::House).active(self.page == Page::Home)
            .on_click(cx.listener(|this, _, _, cx| { this.page = Page::Home; cx.notify(); })),
        // ...
    ])))
```

## Input

```rust
let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search ..."));
cx.subscribe_in(&search, window, |this, state, ev: &InputEvent, _, cx| match ev {
    InputEvent::Change => { this.last_query = state.read(cx).value(); cx.notify(); }
    InputEvent::PressEnter { .. } => { /* run search */ }
    _ => {}   // Focus, Blur
});
// render: Input::new(&self.search).cleanable(true)
// keep returned Subscriptions in the struct (_subscriptions: Vec<Subscription>)
```

## Virtual list (1,000 rows)

```rust
uniform_list("tracks", rows.len(), move |range, _window, cx| {
    range.map(|ix| h_flex().id(ix).w_full().h(px(32.)).child(rows[ix].clone())).collect()
}).flex_1().h_full()
// cx.processor(...) gives &mut Self access; variable heights: component::v_virtual_list
```

## Remote image + cache

```rust
img("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
    .size(px(48.)).rounded(px(4.)).object_fit(ObjectFit::Cover)
    .with_loading(|| div().size(px(48.)).into_any_element())
    .with_fallback(|| div().size(px(48.)).bg(red()).into_any_element())
// on an ancestor: .image_cache(retain_all("covers"))  keeps decoded images across frames
// needs cx.set_http_client(...), otherwise remote URLs go straight to the fallback
```

## Slider

```rust
let volume = cx.new(|_| SliderState::new().min(0.).max(100.).default_value(70.));
cx.subscribe(&volume, |this, _, ev: &SliderEvent, cx| match ev {
    SliderEvent::Change(v) | SliderEvent::Release(v) => { this.volume = v.start(); cx.notify(); }
});
// render: Slider::new(&self.volume)
```

## Tasks

- Foreground: `cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| ..)` -> `Task<R>`;
  store it (drop cancels) or `.detach()`. `this.update(cx, |this, cx| ..)` is Err once dropped.
- Background: `cx.background_spawn(async move { ... })`; sleeps: `cx.background_executor().timer(d)`.

## Backend thread -> UI (wake callback pattern)

```rust
let (event_tx, event_rx) = std::sync::mpsc::channel::<BackendEvent>();
let (wake_tx, wake_rx) = smol::channel::bounded::<()>(1);
spawn_backend(event_tx, move || { let _ = wake_tx.try_send(()); }); // wake: Fn() + Send
self._backend_task = cx.spawn(async move |this, cx| {
    while wake_rx.recv().await.is_ok() {
        let mut latest = None;
        while let Ok(ev) = event_rx.try_recv() { latest = Some(ev); }   // drain, coalesce
        if this.update(cx, |this, cx| { /* apply */ cx.notify(); }).is_err() { break; }
    }
});
// backend stays GPUI-free; bounded(1) + try_send coalesces wakes
```

## Pitfalls hit

- `assets::Assets::load` returns **Err** on unknown paths: chain it last or icons go missing.
- `h_flex()` (component) sets `items_center`; a `v_flex` child collapses vertically and gets
  centred. Give the column `.h_full()` (and `.min_h_0()` on the row) so `flex_1` lists get space.
- `uniform_list` rows size to their content width; add `.w_full()` to rows.
- Monitor in DPMS off: no frame callbacks, so no redraws. `kscreen-doctor --dpms on` first.
- No `.desktop` file for app_id `ytfast-gpui`: KDE shows a generic window icon.
- From a non-session shell, export `WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`.

## Build numbers (debug, `-j4`, deps at opt-level 2)

- Clean build: 9m49s wall (7m14s user; another build was likely sharing the CPU). 864 crates in lock.
- Incremental rebuild of main.rs: ~3 s. Release build not measured yet.
- `target/debug/ytfast-gpui`: 341 MB unstripped; `gpui/target`: 3.4 GB; ~105 MB RSS running.
