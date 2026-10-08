//! The update settings, in `~/.config/encore-yt/updates.json` (the backend's
//! `settings.json` would drop keys it doesn't know when it saves).

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prefs {
    /// Check once a day (on by default).
    #[serde(default = "on")]
    pub check: bool,
    /// Offer pre-releases; unset follows this build: on while it is a
    /// pre-release itself (every release so far is one), off for a stable
    /// build.
    #[serde(default)]
    pub prereleases: Option<bool>,
    /// When the last check answered, in seconds since 1970.
    #[serde(default)]
    pub last_check: u64,
}

fn on() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            check: true,
            prereleases: None,
            last_check: 0,
        }
    }
}

impl Prefs {
    pub fn load(path: &Path) -> Self {
        encore_core::paths::read_json(path).unwrap_or_default()
    }

    pub fn prereleases(&self) -> bool {
        self.prereleases
            .unwrap_or_else(|| super::feed::is_prerelease(super::VERSION))
    }
}
