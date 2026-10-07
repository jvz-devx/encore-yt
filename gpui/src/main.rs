//! ytfast-gpui: a GPUI interface on ytfast's backend. See docs/gpui/PLAN.md.

// No console window behind the app in a Windows release build.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// GPUI's derive macros (Action, IntoElement) name the `gpui` crate.
extern crate gpui_kit as gpui;

mod account;
mod app;
mod assets;
mod desktop;
mod extras;
mod link;
mod nav;
mod pages;
mod playback;
mod sidebar;
mod startup;
mod theme;
mod views;
mod visuals;

#[cfg(test)]
mod ui_tests;

use std::sync::Arc;

use anyhow::anyhow;

fn main() -> anyhow::Result<()> {
    startup::begin();
    // Bundled mpv, yt-dlp and deno win over the system's.
    desktop::tools::add_bundled_to_path();
    let paths = ytfast::paths::Paths::new()?;
    // A running instance takes the message; this process is then done.
    let Some(launch) = desktop::command_line(&paths) else {
        return Ok(());
    };
    fastframe_log::Logging::new("ytfast-gpui", env!("CARGO_PKG_VERSION"))
        .filter("ytfast=info,ytfast_gpui=info,warn")
        .file(paths.cache.join("ytfast-gpui.log"))
        .panic_log(paths.cache.join("panics-gpui.log"))
        .init()
        .map_err(|e| anyhow!("logging: {e}"))?;
    startup::mark(startup::Milestone::Logging);
    // Slow parts of the start, on threads of their own while the platform
    // starts (M17): the desktop's look and the cover art client.
    let asking = theme::ask_desktop();
    let http = std::thread::spawn(|| {
        reqwest_client::ReqwestClient::user_agent(concat!(
            "ytfast-gpui/",
            env!("CARGO_PKG_VERSION")
        ))
    });

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(move |cx| {
            startup::mark(startup::Milestone::Platform);
            gpui_kit::init(cx);
            // Fonts, colours and gpui-component's theme (YTFAST_GPUI_THEME=light).
            theme::init(asking, cx);
            startup::mark(startup::Milestone::Theme);
            // Each area binds its own shortcuts in the "Music" key context
            // and handles them in its `on_actions`.
            pages::bind_keys(cx);
            playback::bind_keys(cx);
            account::bind_keys(cx);
            desktop::bind_keys(cx);
            extras::bind_keys(cx);
            // img("https://...") fetches cover art through this client.
            match http
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            {
                Ok(http) => cx.set_http_client(Arc::new(http)),
                Err(e) => log::warn!("no HTTP client for cover art: {e}"),
            }
            // The window, and the app living on without it (M4).
            desktop::start(paths, launch.link, cx);
        });
    Ok(())
}
