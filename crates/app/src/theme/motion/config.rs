//! The motion settings as saved in `~/.config/encore-yt/motion.json`. Every
//! field has a default, so a file from an older build (or a hand edit
//! missing keys) still loads; a file that doesn't parse gives the defaults.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// How a page arrives when you go somewhere.
    pub pages: PageStyle,
    /// How fast everything moves: 2 is twice as fast, 0.5 half as fast,
    /// 0 is instant.
    pub speed: f32,
    /// Menus and popovers (right-click menus, the sleep timer, the account
    /// menu).
    pub menus: bool,
    /// Panels and dialogs (Settings, Up next, shortcuts, Play anything).
    pub panels: bool,
    /// Toasts above the player bar.
    pub toasts: bool,
    /// Now Playing and Stage opening.
    pub now_playing: bool,
    /// Loading placeholders pulse.
    pub skeleton: bool,
    /// Whether to cut motion back, and who decides.
    pub reduce: Reduce,
    pub lyrics: Lyrics,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pages: PageStyle::Fade,
            speed: 1.0,
            menus: true,
            panels: true,
            toasts: true,
            now_playing: true,
            skeleton: true,
            reduce: Reduce::System,
            lyrics: Lyrics::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PageStyle {
    None,
    Fade,
    Slide,
    Scale,
}

impl PageStyle {
    pub const ALL: [Self; 4] = [Self::None, Self::Fade, Self::Slide, Self::Scale];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Fade => "Fade",
            Self::Slide => "Slide",
            Self::Scale => "Scale",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reduce {
    /// As the desktop asks (the portal's reduced motion, KDE's animation
    /// speed at instant).
    System,
    Always,
    Never,
}

/// How timed lyrics move.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lyrics {
    /// Lines grow, dim and scroll smoothly; off, they change at once.
    pub glide: bool,
    /// The current line's size against the others (1 = the same).
    pub scale: f32,
    /// How far other lines fade (0 none, 1 out of sight); past lines fade
    /// the full amount, upcoming ones less.
    pub dim: f32,
    /// Lines further from the current one fade more.
    pub fade_far: bool,
    /// The current line fills as it is sung.
    pub sweep: bool,
    pub anchor: Anchor,
    pub size: TextSize,
    pub align: Align,
}

impl Default for Lyrics {
    fn default() -> Self {
        Self {
            glide: true,
            scale: 1.1,
            dim: 0.55,
            fade_far: true,
            sweep: true,
            anchor: Anchor::Third,
            size: TextSize::M,
            align: Align::Left,
        }
    }
}

/// Where the current line sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Third,
    Centre,
}

impl Anchor {
    /// From the top, as a share of the view.
    pub fn share(self) -> f32 {
        match self {
            Self::Third => 0.33,
            Self::Centre => 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextSize {
    S,
    M,
    L,
    XL,
}

impl TextSize {
    pub const ALL: [Self; 4] = [Self::S, Self::M, Self::L, Self::XL];

    pub fn label(self) -> &'static str {
        match self {
            Self::S => "S",
            Self::M => "M",
            Self::L => "L",
            Self::XL => "XL",
        }
    }

    /// Against the medium size.
    pub fn factor(self) -> f32 {
        match self {
            Self::S => 0.8,
            Self::M => 1.0,
            Self::L => 1.2,
            Self::XL => 1.42,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Centre,
}

/// The slowest and fastest speeds a saved file may ask for (Settings
/// offers 0.5× to 2×; slower speeds are for hand edits and for capturing
/// a transition mid-way); a value outside them is brought inside.
const SLOWEST: f32 = 0.1;
const FASTEST: f32 = 4.0;

impl Config {
    pub fn load(path: &Path) -> Self {
        encore_core::paths::read_json::<Self>(path)
            .map(Self::sane)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let written = serde_json::to_vec_pretty(self)
            .map_err(std::io::Error::other)
            .and_then(|bytes| encore_core::paths::write_atomic(path, &bytes));
        if let Err(e) = written {
            log::warn!("couldn't save {}: {e}", path.display());
        }
    }

    /// Numbers brought into their ranges.
    fn sane(mut self) -> Self {
        self.speed = if self.speed.is_finite() && self.speed > 0.0 {
            self.speed.clamp(SLOWEST, FASTEST)
        } else {
            0.0
        };
        let l = &mut self.lyrics;
        l.scale = if l.scale.is_finite() {
            l.scale.clamp(1.0, 1.4)
        } else {
            1.0
        };
        l.dim = if l.dim.is_finite() {
            l.dim.clamp(0.0, 0.9)
        } else {
            0.0
        };
        self
    }
}
