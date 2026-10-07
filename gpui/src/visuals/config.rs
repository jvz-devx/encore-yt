//! Settings → Visuals: every effect's switches and parameters, saved in
//! `~/.config/ytfast/visuals.json` (PLAN M21).
//!
//! The file is versioned, every field has a default and unknown fields are
//! ignored, so an older or newer file still loads. Four presets (Off, Calm,
//! Default, Vivid) set the effects' strength; Default is the look the app
//! had before these settings. The visualiser's own choices (style, where it
//! shows, bars, frequencies, colours) and Stage's are kept when a preset is
//! picked.
//!
//! The settings live on the UI thread ([`get`], [`set`]); the effects read
//! them on every render, so a change shows in the next frame. Environment
//! variables override what is saved, without saving: `YTFAST_GPUI_VISUALS=0`
//! (everything off), `YTFAST_GPUI_VISUALS_FPS`, `YTFAST_GPUI_VISUALS_FLIGHT_MS`,
//! `YTFAST_GPUI_VISUALS_PRESET=off|calm|default|vivid` and
//! `YTFAST_GPUI_VISUALIZER=bars|mirrored|ring|line|particles` (also shows it
//! in Now Playing and Stage).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// The file's format version; files from newer builds still load.
pub const VERSION: u32 = 1;
/// The frame rates the cap offers.
pub const FPS: [u32; 4] = [15, 20, 30, 60];
/// The visualiser's bar counts: least, most.
pub const BARS: (u32, u32) = (16, 128);
/// The frequency range the analysis covers, in Hz.
pub const HZ: (f32, f32) = (50., 16_000.);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    Off,
    Calm,
    Default,
    Vivid,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Off, Preset::Calm, Preset::Default, Preset::Vivid];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Off => "Off",
            Preset::Calm => "Calm",
            Preset::Default => "Default",
            Preset::Vivid => "Vivid",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|p| p.label().eq_ignore_ascii_case(s))
    }
}

/// Every effect's settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualsConfig {
    pub version: u32,
    /// The preset last picked ("Reset to preset" goes back to it).
    pub preset: Preset,
    /// Any effect at all (the Off preset turns this off).
    pub on: bool,
    /// The most window frames a second while something animates.
    pub fps: u32,
    pub backdrop: Backdrop,
    pub glow: Glow,
    pub seek: Seek,
    pub halos: Halos,
    pub dissolve: Timed,
    pub flight: Timed,
    pub stage: Stage,
    pub visualizer: Visualizer,
}

/// Now Playing's animated cover backdrop. Strengths are multipliers: 1 is
/// the Default look.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Backdrop {
    pub on: bool,
    pub blur: f32,
    pub swirl: f32,
    pub motes: bool,
    /// How many motes, 0..2.
    pub motes_amount: f32,
    pub mote_size: f32,
    pub bloom: f32,
    pub bass_pulse: f32,
    /// How much of the cover's colour shows.
    pub intensity: f32,
}

/// The player bar's glow of the cover's palette.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Glow {
    pub on: bool,
    pub intensity: f32,
}

/// The shader seek bar's extras.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seek {
    /// The song's loudness under the played part.
    pub waveform: bool,
    /// The most-replayed ridge over the seek bar.
    pub ridge: bool,
}

/// The rings around the play button and the cover on each beat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Halos {
    pub on: bool,
    pub strength: f32,
}

/// An effect that runs once for a while: the cover dissolve, the flight.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timed {
    pub on: bool,
    pub ms: u32,
}

/// Stage (F).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stage {
    /// The cover backdrop behind Stage.
    pub backdrop: bool,
    /// The visualiser in Stage.
    pub visualizer: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Bars,
    Mirrored,
    Ring,
    Line,
    Particles,
}

impl Style {
    pub const ALL: [Style; 5] = [
        Style::Bars,
        Style::Mirrored,
        Style::Ring,
        Style::Line,
        Style::Particles,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Style::Bars => "Bars",
            Style::Mirrored => "Mirrored",
            Style::Ring => "Ring",
            Style::Line => "Line",
            Style::Particles => "Particles",
        }
    }

    /// The shader's style number.
    pub fn index(self) -> u32 {
        self as u32
    }
}

/// What Now Playing shows above the song's title.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// The thin spectrum (the Default look).
    Spectrum,
    Visualizer,
    Both,
    Nothing,
}

impl Placement {
    pub const ALL: [Placement; 4] = [
        Placement::Spectrum,
        Placement::Visualizer,
        Placement::Both,
        Placement::Nothing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Placement::Spectrum => "Spectrum",
            Placement::Visualizer => "Visualiser",
            Placement::Both => "Both",
            Placement::Nothing => "Nothing",
        }
    }

    pub fn spectrum(self) -> bool {
        matches!(self, Placement::Spectrum | Placement::Both)
    }

    pub fn visualizer(self) -> bool {
        matches!(self, Placement::Visualizer | Placement::Both)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Spacing {
    Log,
    Linear,
}

/// Where the visualiser's colours come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Palette {
    /// The cover's colours.
    Cover,
    /// The theme's live colour (`signal`).
    Accent,
    /// The theme's ink (`text`).
    Theme,
    /// Two of [`Swatch`].
    Custom,
}

impl Palette {
    pub const ALL: [Palette; 4] = [
        Palette::Cover,
        Palette::Accent,
        Palette::Theme,
        Palette::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Palette::Cover => "Cover",
            Palette::Accent => "Accent",
            Palette::Theme => "Theme",
            Palette::Custom => "Custom",
        }
    }
}

/// The fixed colours a custom gradient picks from (content colours, like
/// YouTube's mood dots), as OKLCH lightness, chroma and hue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Swatch {
    Rose,
    Coral,
    Amber,
    Lime,
    Mint,
    Teal,
    Sky,
    Indigo,
    Violet,
    Magenta,
    White,
}

impl Swatch {
    pub const ALL: [Swatch; 11] = [
        Swatch::Rose,
        Swatch::Coral,
        Swatch::Amber,
        Swatch::Lime,
        Swatch::Mint,
        Swatch::Teal,
        Swatch::Sky,
        Swatch::Indigo,
        Swatch::Violet,
        Swatch::Magenta,
        Swatch::White,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Swatch::Rose => "Rose",
            Swatch::Coral => "Coral",
            Swatch::Amber => "Amber",
            Swatch::Lime => "Lime",
            Swatch::Mint => "Mint",
            Swatch::Teal => "Teal",
            Swatch::Sky => "Sky",
            Swatch::Indigo => "Indigo",
            Swatch::Violet => "Violet",
            Swatch::Magenta => "Magenta",
            Swatch::White => "White",
        }
    }

    /// OKLCH: lightness, chroma, hue in degrees.
    pub fn oklch(self) -> [f32; 3] {
        match self {
            Swatch::Rose => [0.68, 0.2, 15.],
            Swatch::Coral => [0.72, 0.16, 45.],
            Swatch::Amber => [0.82, 0.16, 80.],
            Swatch::Lime => [0.86, 0.2, 128.],
            Swatch::Mint => [0.84, 0.13, 165.],
            Swatch::Teal => [0.74, 0.12, 195.],
            Swatch::Sky => [0.74, 0.14, 235.],
            Swatch::Indigo => [0.6, 0.19, 275.],
            Swatch::Violet => [0.66, 0.2, 300.],
            Swatch::Magenta => [0.68, 0.24, 335.],
            Swatch::White => [0.97, 0.0, 0.],
        }
    }

    /// Display-space (sRGB) red, green and blue, 0..1.
    pub fn rgb(self) -> [f32; 3] {
        let [l, c, h] = self.oklch();
        let (s, co) = h.to_radians().sin_cos();
        let lab = [l, c * co, c * s];
        ytfast_visuals::color::linear_to_display(ytfast_visuals::color::oklab_to_linear(lab))
    }
}

/// The audio visualiser.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Visualizer {
    pub style: Style,
    pub now_playing: Placement,
    pub bars: u32,
    /// Gain on the levels, 0.5..2.
    pub sensitivity: f32,
    /// How much of the last frame each bar keeps while rising, 0..0.95.
    pub smoothing: f32,
    /// How fast a bar falls, in heights a second.
    pub decay: f32,
    pub low_hz: f32,
    pub high_hz: f32,
    pub spacing: Spacing,
    pub peaks: bool,
    /// How fast a peak cap falls, in heights a second.
    pub peak_fall: f32,
    pub palette: Palette,
    pub custom: [Swatch; 2],
    pub opacity: f32,
    pub glow: f32,
}

impl Default for VisualsConfig {
    fn default() -> Self {
        Self {
            version: VERSION,
            preset: Preset::Default,
            on: true,
            fps: 20,
            backdrop: Backdrop::default(),
            glow: Glow::default(),
            seek: Seek::default(),
            halos: Halos::default(),
            dissolve: Timed { on: true, ms: 900 },
            flight: Timed { on: true, ms: 320 },
            stage: Stage::default(),
            visualizer: Visualizer::default(),
        }
    }
}

impl Default for Backdrop {
    fn default() -> Self {
        Self {
            on: true,
            blur: 1.,
            swirl: 1.,
            motes: true,
            motes_amount: 1.,
            mote_size: 1.,
            bloom: 1.,
            bass_pulse: 1.,
            intensity: 1.,
        }
    }
}

impl Default for Glow {
    fn default() -> Self {
        Self {
            on: true,
            intensity: 1.,
        }
    }
}

impl Default for Seek {
    fn default() -> Self {
        Self {
            waveform: true,
            ridge: true,
        }
    }
}

impl Default for Halos {
    fn default() -> Self {
        Self {
            on: true,
            strength: 1.,
        }
    }
}

impl Default for Timed {
    fn default() -> Self {
        Self { on: true, ms: 600 }
    }
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            backdrop: true,
            visualizer: false,
        }
    }
}

impl Default for Visualizer {
    fn default() -> Self {
        Self {
            style: Style::Bars,
            now_playing: Placement::Spectrum,
            bars: 64,
            sensitivity: 1.,
            smoothing: 0.5,
            decay: 1.6,
            low_hz: HZ.0,
            high_hz: HZ.1,
            spacing: Spacing::Log,
            peaks: true,
            peak_fall: 0.6,
            palette: Palette::Cover,
            custom: [Swatch::Rose, Swatch::Violet],
            opacity: 0.9,
            glow: 0.6,
        }
    }
}

impl VisualsConfig {
    /// The settings `preset` sets, with this one's own choices kept.
    pub fn with_preset(&self, preset: Preset) -> Self {
        let base = Self::default();
        let mut out = Self {
            preset,
            visualizer: Visualizer {
                // Kept: what and where, not how strong.
                sensitivity: base.visualizer.sensitivity,
                smoothing: base.visualizer.smoothing,
                decay: base.visualizer.decay,
                opacity: base.visualizer.opacity,
                glow: base.visualizer.glow,
                ..self.visualizer.clone()
            },
            stage: self.stage.clone(),
            ..base
        };
        match preset {
            Preset::Off => out.on = false,
            Preset::Default => {}
            Preset::Calm => out.calm(),
            Preset::Vivid => out.vivid(),
        }
        out
    }

    fn calm(&mut self) {
        self.fps = 15;
        let b = &mut self.backdrop;
        (b.swirl, b.motes_amount, b.mote_size) = (0.5, 0.5, 0.8);
        (b.bloom, b.bass_pulse, b.intensity) = (0.7, 0.3, 0.8);
        self.glow.intensity = 0.6;
        self.halos.strength = 0.4;
        self.dissolve.ms = 1400;
        self.flight.ms = 420;
        let v = &mut self.visualizer;
        (v.sensitivity, v.smoothing, v.decay) = (0.9, 0.75, 0.8);
        (v.opacity, v.glow) = (0.7, 0.3);
    }

    fn vivid(&mut self) {
        self.fps = 30;
        let b = &mut self.backdrop;
        (b.blur, b.swirl, b.motes_amount, b.mote_size) = (0.8, 1.5, 1.8, 1.3);
        (b.bloom, b.bass_pulse, b.intensity) = (1.6, 1.8, 1.35);
        self.glow.intensity = 1.4;
        self.halos.strength = 1.6;
        self.dissolve.ms = 700;
        let v = &mut self.visualizer;
        (v.sensitivity, v.smoothing, v.decay) = (1.2, 0.35, 2.2);
        (v.opacity, v.glow) = (1., 1.);
    }

    /// Whether these settings are exactly what their preset sets.
    pub fn is_preset(&self) -> bool {
        self.with_preset(self.preset) == *self
    }

    /// Values from a file (or a hand edit) brought into their ranges.
    fn clamped(mut self) -> Self {
        self.version = VERSION;
        if !FPS.contains(&self.fps) {
            self.fps = 20;
        }
        let m = |v: &mut f32, hi: f32| *v = if v.is_finite() { v.clamp(0., hi) } else { 1. };
        let b = &mut self.backdrop;
        for v in [&mut b.blur, &mut b.swirl, &mut b.motes_amount, &mut b.bloom] {
            m(v, 2.);
        }
        for v in [&mut b.mote_size, &mut b.bass_pulse, &mut b.intensity] {
            m(v, 2.);
        }
        m(&mut self.glow.intensity, 2.);
        m(&mut self.halos.strength, 2.);
        self.dissolve.ms = self.dissolve.ms.clamp(200, 3000);
        self.flight.ms = self.flight.ms.clamp(120, 2000);
        let v = &mut self.visualizer;
        v.bars = v.bars.clamp(BARS.0, BARS.1);
        m(&mut v.sensitivity, 3.);
        m(&mut v.smoothing, 0.95);
        m(&mut v.decay, 8.);
        m(&mut v.peak_fall, 4.);
        m(&mut v.opacity, 1.);
        m(&mut v.glow, 2.);
        v.low_hz = v.low_hz.clamp(HZ.0, 2_000.);
        v.high_hz = v.high_hz.clamp(v.low_hz * 2., HZ.1);
        self
    }

    /// The environment's overrides on top (not saved).
    fn overridden(mut self) -> Self {
        let var = |name: &str| std::env::var(name).ok();
        if let Some(preset) = var("YTFAST_GPUI_VISUALS_PRESET").and_then(|p| Preset::parse(&p)) {
            self = self.with_preset(preset);
        }
        if var("YTFAST_GPUI_VISUALS").is_some_and(|v| v == "0") {
            self.on = false;
        }
        if let Some(fps) = var("YTFAST_GPUI_VISUALS_FPS").and_then(|v| v.parse().ok()) {
            self.fps = fps;
        }
        if let Some(ms) = var("YTFAST_GPUI_VISUALS_FLIGHT_MS").and_then(|v| v.parse().ok()) {
            self.flight.ms = ms;
        }
        let style = var("YTFAST_GPUI_VISUALIZER").and_then(|s| {
            Style::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(&s))
        });
        if let Some(style) = style {
            self.visualizer.style = style;
            self.visualizer.now_playing = Placement::Visualizer;
            self.stage.visualizer = true;
        }
        self
    }
}

/// The saved settings and the ones in effect (with the overrides).
#[derive(Default)]
struct Store {
    saved: VisualsConfig,
    effective: Rc<VisualsConfig>,
    path: Option<PathBuf>,
    /// Counts changes, so the effects can tell a still picture is stale.
    revision: u64,
}

thread_local! {
    static STORE: RefCell<Store> = RefCell::new(Store {
        effective: Rc::new(VisualsConfig::default().overridden()),
        ..Store::default()
    });
}

/// Reads `visuals.json` from the config directory `dir`, once.
pub fn load(dir: &Path) {
    let path = dir.join("visuals.json");
    let saved: VisualsConfig = std::fs::read(&path)
        .ok()
        .and_then(
            |bytes| match serde_json::from_slice::<VisualsConfig>(&bytes) {
                Ok(c) => Some(c),
                Err(e) => {
                    log::warn!(
                        "visuals: {} unreadable, using defaults: {e}",
                        path.display()
                    );
                    None
                }
            },
        )
        .unwrap_or_default()
        .clamped();
    STORE.with_borrow_mut(|s| {
        s.effective = Rc::new(saved.clone().overridden());
        s.saved = saved;
        s.path = Some(path);
        s.revision += 1;
    });
}

/// The settings in effect.
pub fn get() -> Rc<VisualsConfig> {
    STORE.with_borrow(|s| s.effective.clone())
}

/// The settings as saved (what Settings shows).
pub fn saved() -> VisualsConfig {
    STORE.with_borrow(|s| s.saved.clone())
}

/// Bumped on every change.
pub fn revision() -> u64 {
    STORE.with_borrow(|s| s.revision)
}

/// New settings, in effect at once; written to the file when `save` (a
/// slider saves when it is let go).
pub fn set(config: VisualsConfig, save: bool) {
    let config = config.clamped();
    let path = STORE.with_borrow_mut(|s| {
        s.effective = Rc::new(config.clone().overridden());
        s.saved = config.clone();
        s.revision += 1;
        s.path.clone()
    });
    if save && let Some(path) = path {
        write(&path, &config);
    }
}

fn write(path: &Path, config: &VisualsConfig) {
    let written = serde_json::to_vec_pretty(config)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            ytfast::paths::write_atomic(path, &bytes)
        });
    if let Err(e) = written {
        log::warn!("visuals: couldn't save {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An old file with fewer fields and one this build doesn't know
    /// loads, keeping what it says and defaulting the rest.
    #[test]
    fn partial_and_unknown_fields_load() {
        let json = r#"{"version": 9, "fps": 30, "future": [1, 2],
            "backdrop": {"bloom": 1.5, "sparkle": true},
            "visualizer": {"style": "ring"}}"#;
        let c: VisualsConfig = serde_json::from_str(json).expect("loads");
        let c = c.clamped();
        assert_eq!(c.fps, 30);
        assert_eq!(c.backdrop.bloom, 1.5);
        assert!(c.backdrop.on);
        assert_eq!(c.visualizer.style, Style::Ring);
        assert_eq!(c.visualizer.bars, 64);
        assert_eq!(c.version, VERSION);
    }

    /// Picking a preset keeps the visualiser's choices, and the Default
    /// preset is the default settings.
    #[test]
    fn presets_keep_the_visualisers_choices() {
        let mut c = VisualsConfig::default();
        assert!(c.is_preset());
        c.visualizer.style = Style::Line;
        c.backdrop.bloom = 2.;
        assert!(!c.is_preset());
        let vivid = c.with_preset(Preset::Vivid);
        assert_eq!(vivid.visualizer.style, Style::Line);
        assert!(vivid.is_preset());
        assert!(!c.with_preset(Preset::Off).on);
        let back = vivid.with_preset(Preset::Default);
        assert_eq!(back.backdrop, Backdrop::default());
    }
}
