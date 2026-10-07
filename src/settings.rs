//! Choices that last across launches, in `~/.config/ytfast/settings.json`.

use serde::{Deserialize, Serialize};

use crate::paths::Paths;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// The browser profile whose YouTube session to use ("google-chrome/Default");
    /// unset means the most recently used signed-in profile.
    #[serde(default)]
    pub browser_profile: Option<String>,
    /// The YouTube channel to act as: a brand account's page id, or `""`
    /// for the Google account's own channel. Unset (or a channel the
    /// account no longer has) means the one YouTube has selected for the
    /// session, as the browser's account switcher left it.
    #[serde(default)]
    pub channel: Option<String>,
    /// Show a desktop notification when the song changes (off by default).
    #[serde(default)]
    pub notifications: bool,
    /// Even out loudness between songs from YouTube's loudness data; unset
    /// means on. See [`Settings::normalizes`].
    #[serde(default)]
    pub normalize: Option<bool>,
    #[serde(default)]
    pub equalizer: crate::equalizer::Equalizer,
    /// Draw covers outside Now Playing and Stage in the theme's colours (off by default).
    #[serde(default)]
    pub paint_covers: bool,
    /// Smooth mixes on radios and mixes (off by default) and its length.
    #[serde(default)]
    pub mixes: crate::model::Mixes,
    /// The audio engine; unset means the default (`YTFAST_PLAYER` overrides it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<crate::player::Kind>,
}

impl Settings {
    /// Missing or damaged settings read as the defaults.
    pub fn load(paths: &Paths) -> Self {
        std::fs::read(paths.config.join("settings.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn normalizes(&self) -> bool {
        self.normalize.unwrap_or(true)
    }

    pub fn save(&self, paths: &Paths) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::paths::write_atomic(&paths.config.join("settings.json"), &bytes)
    }
}
