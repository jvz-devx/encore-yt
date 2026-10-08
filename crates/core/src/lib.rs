//! The backend of Encore, a native YouTube Music client: InnerTube,
//! sign-in, playback and the desktop services. The app is in crates/app.

/// The name people see: window, menus, MPRIS, notifications, tray.
pub const APP_NAME: &str = "Encore";
/// The app id: Wayland app id, desktop file, macOS bundle id.
pub const APP_ID: &str = "io.github.jvz-devx.encore-yt";

/// The id the desktop knows this app by: `APP_ID`, except in a Flatpak, whose
/// id has an underscore in the user part (a hyphen is allowed only in the last
/// segment) and which it exports as `FLATPAK_ID`. The window class, the
/// notification's desktop entry and MPRIS all have to match the installed
/// `.desktop` file.
pub fn app_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        std::env::var("FLATPAK_ID")
            .ok()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| APP_ID.into())
    })
}

pub mod account;
pub mod auth;
pub mod backend;
pub mod browsers;
pub mod desktop;
pub mod discord;
pub mod equalizer;
pub mod heat;
pub mod innertube;
pub mod jsc;
pub mod links;
pub mod lyrics;
pub mod metered;
pub mod migrate;
pub mod model;
pub mod mpris;
pub mod notify;
pub mod parse;
pub mod paths;
pub mod player;
pub mod resolver;
pub mod searches;
pub mod settings;
pub mod single_instance;
pub mod streams;
pub mod tray;
