//! Encore: a native YouTube Music client in GPUI, on the backend in
//! crates/core.

// No console window behind the app in a Windows release build.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(
    elided_lifetimes_in_paths,
    reason = "GPUI callback Context lifetimes follow their borrowed event context; explicit placeholders add no ownership information"
)]

// GPUI's derive macros (Action, IntoElement) name the `gpui` crate.
#[allow(unused_extern_crates, reason = "GPUI derives resolve this crate alias")]
extern crate gpui_kit as gpui;

mod account;
mod app;
mod assets;
mod casting;
mod desktop;
mod extras;
mod likes;
mod link;
mod nav;
mod pages;
mod persistence;
mod playback;
mod settings;
mod sidebar;
mod sign_in;
mod sign_in_window;
mod startup;
mod theme;
mod update;
mod views;
mod visuals;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod ui_tests;

use std::sync::Arc;

use anyhow::anyhow;

fn main() -> anyhow::Result<()> {
    startup::begin();
    // The update helper runs here and exits; a start after an update keeps
    // its receipt (M16).
    let args = update::intercept();
    let paths = encore_core::paths::Paths::new()?;
    // A running instance takes the message; this process is then done.
    let Some(launch) = desktop::command_line(&paths, args) else {
        return Ok(());
    };
    fastframe_log::Logging::new("encore-yt", update::VERSION)
        .filter("encore_core=info,encore_yt=info,encore_audio=info,encore_cast=info,warn")
        .file(paths.cache.join("encore-yt.log"))
        .panic_log(paths.cache.join("panics.log"))
        .init()
        .map_err(|e| anyhow!("logging: {e}"))?;
    if let Some(note) = encore_core::paths::migration_note() {
        log::info!("{note}");
    }
    startup::mark(startup::Milestone::Logging);
    // Slow parts of the start, on threads of their own while the platform
    // and the window start (M17): the desktop's look, the cover art client
    // and the backend.
    let asking = theme::ask_desktop();
    let http = std::thread::spawn(|| {
        reqwest_client::ReqwestClient::user_agent(concat!("encore-yt/", env!("CARGO_PKG_VERSION")))
    });
    let early = app::Early::start(paths.clone())?;
    let motion_path = paths.config.join("motion.json");

    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(move |cx| {
            startup::mark(startup::Milestone::Platform);
            gpui_kit::init(cx);
            // Settings → Motion, before the theme applies the desktop's
            // reduced motion through it.
            theme::motion::init(motion_path, cx);
            // Fonts, colours and gpui-component's theme (ENCORE_THEME=light).
            theme::init(asking, cx);
            startup::mark(startup::Milestone::Theme);
            // Each area binds its own shortcuts in the "Music" key context
            // and handles them in its `on_actions`.
            pages::bind_keys(cx);
            playback::bind_keys(cx);
            account::bind_keys(cx);
            desktop::bind_keys(cx);
            extras::bind_keys(cx);
            settings::bind_keys(cx);
            // img("https://...") fetches cover art through this client.
            match http
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            {
                Ok(http) => cx.set_http_client(Arc::new(http)),
                Err(e) => log::warn!("no HTTP client for cover art: {e}"),
            }
            // The window, and the app living on without it (M4).
            desktop::start(paths, early, launch.link, cx);
        });
    Ok(())
}
