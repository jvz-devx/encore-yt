//! Settings → Visuals' sliders: what each one sets, its range and how its
//! value reads, and their states, kept while the app runs.

use std::cell::Cell;
use std::collections::HashMap;

use gpui_kit::component::slider::{Slider, SliderEvent, SliderScale, SliderState, SliderValue};
use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::Colors;
use crate::visuals::config::{self, VisualsConfig};

/// The slider's width in a row.
const WIDTH: Pixels = px(184.);

/// A slider of Settings → Visuals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Knob {
    Blur,
    Swirl,
    Bloom,
    BassPulse,
    Intensity,
    Glow,
    Halos,
    Dissolve,
    Flight,
    Bars,
    Sensitivity,
    Smoothing,
    Decay,
    Frequencies,
    PeakFall,
    Opacity,
    VisGlow,
    Thickness,
    Amount,
    Size,
    Softness,
    Brightness,
    Speed,
    Depth,
    Twinkle,
    TwinkleSpeed,
    Direction,
    Reaction,
    WaveStrength,
    WaveSpeed,
    WaveHeight,
    SceneStrength,
    SceneDetail,
    SceneResolution,
    SceneReaction,
    ScrimStrength,
}

impl Knob {
    const ALL: [Knob; 36] = [
        Knob::Blur,
        Knob::Swirl,
        Knob::Bloom,
        Knob::BassPulse,
        Knob::Intensity,
        Knob::Glow,
        Knob::Halos,
        Knob::Dissolve,
        Knob::Flight,
        Knob::Bars,
        Knob::Sensitivity,
        Knob::Smoothing,
        Knob::Decay,
        Knob::Frequencies,
        Knob::PeakFall,
        Knob::Opacity,
        Knob::VisGlow,
        Knob::Thickness,
        Knob::Amount,
        Knob::Size,
        Knob::Softness,
        Knob::Brightness,
        Knob::Speed,
        Knob::Depth,
        Knob::Twinkle,
        Knob::TwinkleSpeed,
        Knob::Direction,
        Knob::Reaction,
        Knob::WaveStrength,
        Knob::WaveSpeed,
        Knob::WaveHeight,
        Knob::SceneStrength,
        Knob::SceneDetail,
        Knob::SceneResolution,
        Knob::SceneReaction,
        Knob::ScrimStrength,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Knob::Blur => "Blur",
            Knob::Swirl => "Swirl speed",
            Knob::Amount => "Amount",
            Knob::Size => "Size",
            Knob::Softness => "Softness",
            Knob::Brightness => "Brightness",
            Knob::Speed | Knob::WaveSpeed => "Speed",
            Knob::Depth => "Depth",
            Knob::Twinkle => "Twinkle",
            Knob::TwinkleSpeed => "Twinkle speed",
            Knob::Direction => "Direction",
            Knob::Reaction | Knob::SceneReaction => "Music reaction",
            Knob::WaveStrength | Knob::SceneStrength | Knob::ScrimStrength => "Strength",
            Knob::SceneDetail => "Detail",
            Knob::SceneResolution => "Resolution",
            Knob::WaveHeight => "Height",
            Knob::Bloom => "Bloom",
            Knob::BassPulse => "Bass pulse",
            Knob::Intensity => "Colour",
            Knob::Glow | Knob::VisGlow => "Glow",
            Knob::Halos => "Strength",
            Knob::Dissolve | Knob::Flight => "Length",
            Knob::Bars => "Bars",
            Knob::Sensitivity => "Sensitivity",
            Knob::Smoothing => "Smoothing",
            Knob::Decay => "Fall speed",
            Knob::Frequencies => "Frequencies",
            Knob::PeakFall => "Peak fall speed",
            Knob::Opacity => "Opacity",
            Knob::Thickness => "Thickness",
        }
    }

    /// Least, most, step, and whether the scale is logarithmic.
    fn range(self) -> (f32, f32, f32, bool) {
        match self {
            Knob::Size => (config::SPARKLE_PX.0, config::SPARKLE_PX.1, 0.05, false),
            Knob::Softness | Knob::Depth | Knob::Twinkle | Knob::Reaction => (0., 1., 0.05, false),
            Knob::Speed | Knob::WaveSpeed => (0., 3., 0.05, false),
            Knob::TwinkleSpeed => (0.2, 3., 0.05, false),
            Knob::Direction => (0., 360., 5., false),
            Knob::WaveHeight => (0.1, 0.9, 0.01, false),
            Knob::Dissolve => (200., 3000., 50., false),
            Knob::Flight => (120., 2000., 20., false),
            Knob::Bars => (16., 128., 4., false),
            Knob::Sensitivity => (0.5, 3., 0.05, false),
            Knob::Smoothing => (0., 0.95, 0.05, false),
            Knob::Decay => (0.3, 8., 0.1, false),
            Knob::Frequencies => (config::HZ.0, config::HZ.1, 1., true),
            Knob::PeakFall => (0.1, 4., 0.05, false),
            Knob::Opacity => (0.1, 1., 0.05, false),
            Knob::Thickness => (config::THICKNESS.0, config::THICKNESS.1, 0.25, false),
            Knob::SceneStrength => (0.4, 1.5, 0.05, false),
            Knob::SceneDetail | Knob::SceneResolution => (0.5, 1.5, 0.05, false),
            Knob::SceneReaction => (0., 1., 0.05, false),
            Knob::ScrimStrength => (0.5, 1.5, 0.05, false),
            _ => (0., 2., 0.05, false),
        }
    }

    pub fn get(self, c: &VisualsConfig) -> SliderValue {
        let (b, v, p, w) = (&c.backdrop, &c.visualizer, &c.particles, &c.wave);
        SliderValue::Single(match self {
            Knob::Blur => b.blur,
            Knob::Swirl => b.swirl,
            Knob::Amount => p.amount,
            Knob::Size => return SliderValue::Range(p.size_min, p.size_max),
            Knob::Softness => p.softness,
            Knob::Brightness => p.brightness,
            Knob::Speed => p.speed,
            Knob::Depth => p.depth,
            Knob::Twinkle => p.twinkle,
            Knob::TwinkleSpeed => p.twinkle_speed,
            Knob::Direction => p.direction,
            Knob::Reaction => p.reaction,
            Knob::WaveStrength => w.strength,
            Knob::WaveSpeed => w.speed,
            Knob::WaveHeight => w.height,
            Knob::Bloom => b.bloom,
            Knob::BassPulse => b.bass_pulse,
            Knob::Intensity => b.intensity,
            Knob::Glow => c.glow.intensity,
            Knob::Halos => c.halos.strength,
            Knob::Dissolve => c.dissolve.ms as f32,
            Knob::Flight => c.flight.ms as f32,
            Knob::Bars => v.bars as f32,
            Knob::Sensitivity => v.sensitivity,
            Knob::Smoothing => v.smoothing,
            Knob::Decay => v.decay,
            Knob::Frequencies => return SliderValue::Range(v.low_hz, v.high_hz),
            Knob::PeakFall => v.peak_fall,
            Knob::Opacity => v.opacity,
            Knob::VisGlow => v.glow,
            Knob::Thickness => v.thickness,
            Knob::SceneStrength => c.scenes.strength,
            Knob::SceneDetail => c.scenes.detail,
            Knob::SceneResolution => c.scenes.resolution,
            Knob::SceneReaction => c.scenes.reaction,
            Knob::ScrimStrength => c.scenes.scrim_strength,
        })
    }

    fn set(self, c: &mut VisualsConfig, value: SliderValue) {
        let x = value.start();
        let (b, v) = (&mut c.backdrop, &mut c.visualizer);
        let (p, w) = (&mut c.particles, &mut c.wave);
        match self {
            Knob::Blur => b.blur = x,
            Knob::Swirl => b.swirl = x,
            Knob::Amount => p.amount = x,
            Knob::Size => (p.size_min, p.size_max) = (value.start(), value.end()),
            Knob::Softness => p.softness = x,
            Knob::Brightness => p.brightness = x,
            Knob::Speed => p.speed = x,
            Knob::Depth => p.depth = x,
            Knob::Twinkle => p.twinkle = x,
            Knob::TwinkleSpeed => p.twinkle_speed = x,
            Knob::Direction => p.direction = x,
            Knob::Reaction => p.reaction = x,
            Knob::WaveStrength => w.strength = x,
            Knob::WaveSpeed => w.speed = x,
            Knob::WaveHeight => w.height = x,
            Knob::Bloom => b.bloom = x,
            Knob::BassPulse => b.bass_pulse = x,
            Knob::Intensity => b.intensity = x,
            Knob::Glow => c.glow.intensity = x,
            Knob::Halos => c.halos.strength = x,
            Knob::Dissolve => c.dissolve.ms = x.round() as u32,
            Knob::Flight => c.flight.ms = x.round() as u32,
            Knob::Bars => v.bars = x.round() as u32,
            Knob::Sensitivity => v.sensitivity = x,
            Knob::Smoothing => v.smoothing = x,
            Knob::Decay => v.decay = x,
            Knob::Frequencies => {
                // At least an octave apart.
                v.low_hz = value.start().min(2_000.);
                v.high_hz = value.end().max(v.low_hz * 2.);
            }
            Knob::PeakFall => v.peak_fall = x,
            Knob::Opacity => v.opacity = x,
            Knob::VisGlow => v.glow = x,
            Knob::Thickness => v.thickness = x,
            Knob::SceneStrength => c.scenes.strength = x,
            Knob::SceneDetail => c.scenes.detail = x,
            Knob::SceneResolution => c.scenes.resolution = x,
            Knob::SceneReaction => c.scenes.reaction = x,
            Knob::ScrimStrength => c.scenes.scrim_strength = x,
        }
    }

    /// The value as the row reads it.
    pub fn shown(self, c: &VisualsConfig) -> String {
        let value = self.get(c);
        let x = value.start();
        match self {
            Knob::Dissolve | Knob::Flight => format!("{x:.0} ms"),
            Knob::Bars => format!("{x:.0}"),
            Knob::Decay => format!("Falls in {:.1} s", 1. / x.max(0.01)),
            Knob::PeakFall => format!("Caps fall in {:.1} s", 1. / x.max(0.01)),
            Knob::Frequencies => format!("{} to {}", hz(value.start()), hz(value.end())),
            Knob::Size => format!("{:.1} to {:.1} px", value.start(), value.end()),
            Knob::Thickness => format!("{} px", (x * 100.).round() / 100.),
            Knob::Direction => direction(x),
            Knob::WaveHeight => format!("{:.0}% down", x * 100.),
            Knob::TwinkleSpeed => format!("{x:.1}x"),
            _ => format!("{:.0}%", x * 100.),
        }
    }
}

/// A drift direction in words, with its angle.
fn direction(degrees: f32) -> String {
    let names = [
        "Right",
        "Up and right",
        "Up",
        "Up and left",
        "Left",
        "Down and left",
        "Down",
        "Down and right",
    ];
    let i = ((degrees / 45.).round() as usize) % 8;
    format!("{} ({degrees:.0}°)", names[i])
}

fn hz(v: f32) -> String {
    if v >= 1000. {
        format!("{:.1} kHz", v / 1000.)
    } else {
        format!("{v:.0} Hz")
    }
}

/// The section's state: the sliders and which one is held.
pub struct Knobs {
    sliders: HashMap<Knob, Entity<SliderState>>,
    held: Cell<Option<Knob>>,
    _subscriptions: Vec<Subscription>,
}

impl Global for Knobs {}

impl Knobs {
    /// Makes the sliders the first time Settings shows them, and moves
    /// each one that isn't held to the value in effect (a preset changes
    /// many at once).
    pub fn sync(window: &mut Window, cx: &mut Context<MusicApp>) {
        if !cx.has_global::<Knobs>() {
            let knobs = Self::new(cx);
            cx.set_global(knobs);
        }
        let saved = config::saved();
        let held = cx.global::<Knobs>().held.get();
        let sliders: Vec<_> = cx
            .global::<Knobs>()
            .sliders
            .iter()
            .filter(|(k, _)| Some(**k) != held)
            .map(|(k, s)| (*k, s.clone()))
            .collect();
        for (knob, slider) in sliders {
            let want = knob.get(&saved);
            if !close(slider.read(cx).value(), want) {
                slider.update(cx, |s, cx| s.set_value(want, window, cx));
            }
        }
    }

    fn new(cx: &mut Context<MusicApp>) -> Self {
        let saved = config::saved();
        let mut sliders = HashMap::new();
        let mut subscriptions = Vec::new();
        for knob in Knob::ALL {
            let (min, max, step, log) = knob.range();
            let slider = cx.new(|_| {
                // The top first: the default range ends at 100.
                SliderState::new()
                    .max(max)
                    .min(min)
                    .step(step)
                    .scale(if log {
                        SliderScale::Logarithmic
                    } else {
                        SliderScale::Linear
                    })
                    .default_value(knob.get(&saved))
            });
            subscriptions.push(cx.subscribe(&slider, move |_, _, event: &SliderEvent, cx| {
                let (value, done) = match event {
                    SliderEvent::Change(v) => (*v, false),
                    SliderEvent::Release(v) => (*v, true),
                };
                if let Some(knobs) = cx.try_global::<Knobs>() {
                    knobs.held.set((!done).then_some(knob));
                }
                let mut saved = config::saved();
                knob.set(&mut saved, value);
                config::set(saved, done);
                cx.notify();
            }));
            sliders.insert(knob, slider);
        }
        Self {
            sliders,
            held: Cell::new(None),
            _subscriptions: subscriptions,
        }
    }
}

fn close(a: SliderValue, b: SliderValue) -> bool {
    (a.start() - b.start()).abs() < 1e-3 && (a.end() - b.end()).abs() < 1e-3
}

/// A slider row: its name, its value under it, the slider at the right.
pub fn row(knob: Knob, enabled: bool, c: &Colors, cx: &App) -> AnyElement {
    let saved = config::saved();
    let slider = cx
        .try_global::<Knobs>()
        .and_then(|k| k.sliders.get(&knob).cloned());
    let control = div().w(WIDTH).flex_none().children(slider.map(|s| {
        super::super::keyed::slider(knob.label(), &s, enabled, c).child(
            Slider::new(&s)
                .disabled(!enabled)
                .bg(if enabled { c.signal } else { c.text_faint })
                .text_color(c.text),
        )
    }));
    super::super::row(knob.label(), Some(knob.shown(&saved).into()), control, c)
}
