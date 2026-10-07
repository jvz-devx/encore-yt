//! The backend of ytfast-gpui, a native YouTube Music client: InnerTube,
//! sign-in, playback and the desktop services. The app is in gpui/.

pub mod account;
pub mod auth;
pub mod backend;
pub mod desktop;
pub mod equalizer;
pub mod heat;
pub mod innertube;
pub mod jsc;
pub mod links;
pub mod lyrics;
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
