//! ytfast-gpui: a GPUI interface on ytfast's backend. See docs/gpui/PLAN.md.

mod account;
mod app;
mod assets;
mod desktop;
mod extras;
mod nav;
mod pages;
mod playback;
mod views;

use std::sync::Arc;

use anyhow::anyhow;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

fn main() -> anyhow::Result<()> {
    let paths = ytfast::paths::Paths::new()?;
    fastframe_log::Logging::new("ytfast-gpui", env!("CARGO_PKG_VERSION"))
        .filter("ytfast=info,ytfast_gpui=info,warn")
        .file(paths.cache.join("ytfast-gpui.log"))
        .panic_log(paths.cache.join("panics-gpui.log"))
        .init()
        .map_err(|e| anyhow!("logging: {e}"))?;

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            // img("https://...") fetches cover art through this client.
            match reqwest_client::ReqwestClient::user_agent(concat!(
                "ytfast-gpui/",
                env!("CARGO_PKG_VERSION")
            )) {
                Ok(http) => cx.set_http_client(Arc::new(http)),
                Err(e) => log::warn!("no HTTP client for cover art: {e}"),
            }
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("Music".into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
                window_min_size: Some(size(px(900.), px(600.))),
                app_id: Some("ytfast-gpui".into()),
                ..Default::default()
            };
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| app::MusicApp::new(paths, window, cx))
            });
            if let Err(e) = opened {
                log::error!("opening the window: {e}");
                cx.quit();
                return;
            }
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.activate(true);
        });
    Ok(())
}
