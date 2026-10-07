//! Settings → Visuals: every effect's switches and parameters, saved in
//! `~/.config/encore-yt/visuals.json` (PLAN M21).
//!
//! The file is versioned, every field has a default and unknown fields are
//! ignored, so an older or newer file still loads. Four presets (Off, Calm,
//! Default, Vivid) set the effects' strength: Default is the backdrop as the
//! app had it before these settings with a quieter glow and quieter halos
//! on the player bar; Vivid has the bar's glow and halos as before and a
//! livelier backdrop. The
//! visualiser's own choices (style, where it shows, bars, frequencies,
//! colours), Stage's and the frame rate are kept when a preset is picked.
//!
//! The settings live on the UI thread ([`get`], [`set`]); the effects read
//! them on every render, so a change shows in the next frame. Environment
//! variables override what is saved, without saving: `ENCORE_VISUALS=0`
//! (everything off), `ENCORE_VISUALS_FPS`, `ENCORE_VISUALS_FLIGHT_MS`,
//! `ENCORE_VISUALS_PRESET=off|calm|default|vivid` and
//! `ENCORE_VISUALIZER=bars|mirrored|ring|line|particles|scope` (also
//! shows it in Now Playing and Stage) and `ENCORE_SCOPE=mono|stereo|xy`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// The file's format version; files from newer builds still load.
/// Version 2: the frame rate offers 120 and the display's rate, and its
/// default depends on the system ([`default_fps`]).
pub const VERSION: u32 = 2;
/// The frame rate that follows the display (`fps` in the file).
pub const DISPLAY_FPS: u32 = 0;
/// The frame rates Settings offers; [`DISPLAY_FPS`] is the display's rate.
pub const FPS: [u32; 6] = [15, 20, 30, 60, 120, DISPLAY_FPS];

/// The frame rate out of the box: the display's on macOS (a frame costs
/// Apple's GPUs little), 30 on Windows, and 20 on Linux, where the
/// integrated GPUs it was measured on spend 6-10 ms on each window frame
/// (docs/gpui/VISUALS.md, "Frame rate"). The sparkles drift well under a
/// pixel a frame at 20, so motion still looks fluid.
pub fn default_fps() -> u32 {
    if cfg!(target_os = "macos") {
        DISPLAY_FPS
    } else if cfg!(target_os = "windows") {
        30
    } else {
        20
    }
}
/// The sparkles' radius in device pixels: least, most.
pub const SPARKLE_PX: (f32, f32) = (0.5, 4.);
/// The visualiser's bar counts: least, most.
pub const BARS: (u32, u32) = (16, 128);
/// The frequency range the analysis covers, in Hz.
pub const HZ: (f32, f32) = (50., 16_000.);
/// The line's and the scope's stroke, in points.
pub const THICKNESS: (f32, f32) = (1., 6.);

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

    /// What the preset does, in a line (Settings, Play anything).
    pub fn summary(self) -> &'static str {
        match self {
            Preset::Off => "No effects: plain backgrounds and no visualiser",
            Preset::Calm => "Slower, softer effects",
            Preset::Default => "The effects as they come",
            Preset::Vivid => "More colour, more motion",
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
    /// The most window frames a second while something animates, or
    /// [`DISPLAY_FPS`] for the display's rate.
    pub fps: u32,
    pub backdrop: Backdrop,
    pub particles: Particles,
    pub wave: Wave,
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
    pub bloom: f32,
    pub bass_pulse: f32,
    /// How much of the cover's colour shows.
    pub intensity: f32,
}

/// The fine sparkles drifting over the backdrop (its ambient layer).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Particles {
    pub on: bool,
    /// How many, 0..2.
    pub amount: f32,
    /// The far and the near ones' radius, in device pixels.
    pub size_min: f32,
    pub size_max: f32,
    /// 0 a crisp dot, 1 a soft glow.
    pub softness: f32,
    pub brightness: f32,
    /// How fast they drift, 0..3.
    pub speed: f32,
    /// How much size, brightness and speed differ with depth, 0..1.
    pub depth: f32,
    /// How much they fade in and out (0..1), and how fast (0.2..3).
    pub twinkle: f32,
    pub twinkle_speed: f32,
    /// Where they drift, in degrees: 0 right, 90 up.
    pub direction: f32,
    /// How much the music lifts their brightness, 0..1.
    pub reaction: f32,
    pub colour: ParticleColour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParticleColour {
    White,
    /// Tinted with the cover's colours.
    Cover,
    /// Tinted with the theme's live colour.
    Accent,
}

impl ParticleColour {
    pub const ALL: [Self; 3] = [Self::White, Self::Cover, Self::Accent];

    pub fn label(self) -> &'static str {
        match self {
            Self::White => "White",
            Self::Cover => "Cover",
            Self::Accent => "Accent",
        }
    }
}

/// The soft light wave the particles gather round, after the PS3's
/// XrossMediaBar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Wave {
    pub on: bool,
    pub strength: f32,
    /// How fast it undulates, 0..3 (times the backdrop's swirl).
    pub speed: f32,
    /// How many ribbons, 1..3.
    pub ribbons: u32,
    /// Where its middle runs: 0 the top, 1 the bottom.
    pub height: f32,
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            on: true,
            amount: 1.,
            size_min: 0.7,
            size_max: 1.4,
            softness: 0.8,
            brightness: 1.,
            speed: 1.,
            depth: 1.,
            twinkle: 0.6,
            twinkle_speed: 1.,
            direction: 0.,
            reaction: 0.5,
            colour: ParticleColour::Cover,
        }
    }
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            on: true,
            strength: 1.,
            speed: 1.,
            ribbons: 3,
            height: 0.56,
        }
    }
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
    /// The oscilloscope (M22): the samples, not the spectrum.
    Scope,
}

impl Style {
    pub const ALL: [Style; 6] = [
        Style::Bars,
        Style::Mirrored,
        Style::Ring,
        Style::Line,
        Style::Particles,
        Style::Scope,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Style::Bars => "Bars",
            Style::Mirrored => "Mirrored",
            Style::Ring => "Ring",
            Style::Line => "Line",
            Style::Particles => "Particles",
            Style::Scope => "Scope",
        }
    }

    /// The shader's style number.
    pub fn index(self) -> u32 {
        self as u32
    }

    /// Drawn from the spectrum's bars (all but the scope).
    pub fn spectral(self) -> bool {
        self != Style::Scope
    }

    /// Drawn with a stroke whose thickness the settings choose.
    pub fn stroked(self) -> bool {
        matches!(self, Style::Line | Style::Scope)
    }
}

/// How the scope shows the two channels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScopeChannels {
    /// One trace of the mix.
    #[default]
    Mono,
    /// The left channel over the right.
    Stereo,
    /// Mid against side, a goniometer's figure.
    Xy,
}

impl ScopeChannels {
    pub const ALL: [ScopeChannels; 3] = [
        ScopeChannels::Mono,
        ScopeChannels::Stereo,
        ScopeChannels::Xy,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ScopeChannels::Mono => "Mono",
            ScopeChannels::Stereo => "Stereo",
            ScopeChannels::Xy => "X/Y",
        }
    }

    pub fn visuals(self) -> encore_visuals::Channels {
        match self {
            ScopeChannels::Mono => encore_visuals::Channels::Mono,
            ScopeChannels::Stereo => encore_visuals::Channels::Stereo,
            ScopeChannels::Xy => encore_visuals::Channels::XY,
        }
    }
}

/// What Now Playing shows above the song's title: one music graphic, never
/// two stacked (a file from before that offered both gets the visualiser).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// The thin spectrum (the Default look).
    Spectrum,
    #[serde(alias = "both")]
    Visualizer,
    Nothing,
}

impl Placement {
    pub const ALL: [Placement; 3] = [
        Placement::Spectrum,
        Placement::Visualizer,
        Placement::Nothing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Placement::Spectrum => "Spectrum",
            Placement::Visualizer => "Visualiser",
            Placement::Nothing => "Nothing",
        }
    }

    pub fn spectrum(self) -> bool {
        self == Placement::Spectrum
    }

    pub fn visualizer(self) -> bool {
        self == Placement::Visualizer
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
        encore_visuals::color::linear_to_display(encore_visuals::color::oklab_to_linear(lab))
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
    /// The line's and the scope's stroke, in points.
    pub thickness: f32,
    /// How the scope shows the channels.
    pub channels: ScopeChannels,
}

impl Default for VisualsConfig {
    fn default() -> Self {
        Self {
            version: VERSION,
            preset: Preset::Default,
            on: true,
            fps: default_fps(),
            backdrop: Backdrop::default(),
            particles: Particles::default(),
            wave: Wave::default(),
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
            intensity: 0.55,
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
            strength: 0.5,
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
            opacity: 0.85,
            glow: 0.4,
            thickness: 2.5,
            channels: ScopeChannels::Mono,
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
            particles: Particles {
                // Kept: their look, not how many or how strong.
                size_min: self.particles.size_min,
                size_max: self.particles.size_max,
                softness: self.particles.softness,
                depth: self.particles.depth,
                direction: self.particles.direction,
                colour: self.particles.colour,
                ..base.particles.clone()
            },
            wave: Wave {
                ribbons: self.wave.ribbons,
                height: self.wave.height,
                ..base.wave.clone()
            },
            stage: self.stage.clone(),
            fps: self.fps,
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
        let b = &mut self.backdrop;
        (b.swirl, b.bloom, b.bass_pulse, b.intensity) = (0.6, 0.6, 0.4, 0.85);
        let p = &mut self.particles;
        (p.amount, p.brightness, p.speed, p.reaction) = (0.6, 0.7, 0.6, 0.3);
        (self.wave.strength, self.wave.speed) = (0.6, 0.6);
        self.glow.intensity = 0.4;
        self.halos.strength = 0.3;
        self.dissolve.ms = 1400;
        self.flight.ms = 420;
        let v = &mut self.visualizer;
        (v.sensitivity, v.smoothing, v.decay) = (0.9, 0.75, 0.8);
        (v.opacity, v.glow) = (0.7, 0.2);
    }

    /// More of everything: the bar's glow and halos as strong as before
    /// Settings → Visuals, the backdrop livelier than that.
    fn vivid(&mut self) {
        let b = &mut self.backdrop;
        (b.blur, b.swirl, b.bloom) = (0.9, 1.3, 1.3);
        (b.bass_pulse, b.intensity) = (1.4, 1.2);
        let p = &mut self.particles;
        (p.amount, p.brightness, p.speed, p.reaction) = (1.4, 1.2, 1.3, 0.7);
        (self.wave.strength, self.wave.speed) = (1.4, 1.3);
        self.glow.intensity = 1.;
        self.halos.strength = 1.;
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
        // Version 1's default of 20 is the system's default now; rates this
        // build doesn't offer go to it too.
        if !FPS.contains(&self.fps) || (self.version < 2 && self.fps == 20) {
            self.fps = default_fps();
        }
        self.version = VERSION;
        let m = |v: &mut f32, hi: f32| *v = if v.is_finite() { v.clamp(0., hi) } else { 1. };
        let b = &mut self.backdrop;
        for v in [
            &mut b.blur,
            &mut b.swirl,
            &mut b.bloom,
            &mut b.bass_pulse,
            &mut b.intensity,
        ] {
            m(v, 2.);
        }
        let within = |v: &mut f32, (lo, hi): (f32, f32), or: f32| {
            *v = if v.is_finite() { v.clamp(lo, hi) } else { or };
        };
        let p = &mut self.particles;
        for v in [&mut p.amount, &mut p.brightness] {
            m(v, 2.);
        }
        m(&mut p.speed, 3.);
        for v in [
            &mut p.softness,
            &mut p.depth,
            &mut p.twinkle,
            &mut p.reaction,
        ] {
            m(v, 1.);
        }
        within(&mut p.size_min, SPARKLE_PX, 0.7);
        within(&mut p.size_max, SPARKLE_PX, 1.4);
        p.size_max = p.size_max.max(p.size_min);
        within(&mut p.twinkle_speed, (0.2, 3.), 1.);
        within(&mut p.direction, (0., 360.), 0.);
        let w = &mut self.wave;
        m(&mut w.strength, 2.);
        m(&mut w.speed, 3.);
        within(&mut w.height, (0.1, 0.9), 0.56);
        w.ribbons = w.ribbons.clamp(1, 3);
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
        within(&mut v.thickness, THICKNESS, 2.5);
        v.low_hz = v.low_hz.clamp(HZ.0, 2_000.);
        v.high_hz = v.high_hz.clamp(v.low_hz * 2., HZ.1);
        self
    }

    /// The environment's overrides on top (not saved).
    fn overridden(mut self) -> Self {
        let var = |name: &str| std::env::var(name).ok();
        if let Some(preset) = var("ENCORE_VISUALS_PRESET").and_then(|p| Preset::parse(&p)) {
            self = self.with_preset(preset);
        }
        if var("ENCORE_VISUALS").is_some_and(|v| v == "0") {
            self.on = false;
        }
        if let Some(fps) = var("ENCORE_VISUALS_FPS").and_then(|v| v.parse().ok()) {
            self.fps = fps;
        }
        if let Some(ms) = var("ENCORE_VISUALS_FLIGHT_MS").and_then(|v| v.parse().ok()) {
            self.flight.ms = ms;
        }
        let style = var("ENCORE_VISUALIZER").and_then(|s| {
            Style::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(&s))
        });
        if let Some(style) = style {
            self.visualizer.style = style;
            self.visualizer.now_playing = Placement::Visualizer;
            self.stage.visualizer = true;
        }
        let channels = var("ENCORE_SCOPE").and_then(|s| {
            ScopeChannels::ALL
                .into_iter()
                .find(|c| c.label().replace('/', "").eq_ignore_ascii_case(&s))
        });
        if let Some(channels) = channels {
            self.visualizer.channels = channels;
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
            encore_core::paths::write_atomic(path, &bytes)
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
