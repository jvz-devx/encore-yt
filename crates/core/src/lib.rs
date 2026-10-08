//! The backend of Encore, a native YouTube Music client: InnerTube,
//! sign-in, playback and the desktop services. The app is in crates/app.

/// The name people see: window, menus, MPRIS, notifications, tray.
pub const APP_NAME: &str = "Encore";
/// The app id: Wayland app id, desktop file, macOS bundle id.
pub const APP_ID: &str = "io.github.jvz-devx.encore-yt";

pub mod account;
pub mod auth;
pub mod backend;
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
