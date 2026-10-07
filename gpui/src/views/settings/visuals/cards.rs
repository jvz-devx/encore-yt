//! The cards inside Settings → Visuals' tabs: one per effect, its name and
//! a line on how it is set, its switch, and its sliders and choices.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::super::overlays::keycap::combo;
use super::super::super::widgets;
use super::knobs::{self, Knob};
use super::{change, labelled, toggle};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, space};
use crate::visuals::config::{ParticleColour, VisualsConfig};

/// An effect's card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Card {
    Backdrop,
    Stage,
    Particles,
    Wave,
    Glow,
    Halos,
    Seek,
    Visualizer,
    Dissolve,
    Flight,
}

/// A card: its header (name, what it is set to, its switch) over its
/// settings, which grey out while it is off.
pub fn card(k: Card, s: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let on = switched(k, s);
    let switch = on.map(|on| {
        widgets::switch(("visuals-switch", k as usize), on, c).on_click(cx.listener(
            move |_, on: &bool, _, cx| {
                let on = *on;
                change(cx, |s| set_switch(k, s, on));
            },
        ))
    });
    let header = h_flex()
        .py(space::SM)
        .gap(space::MD)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(div().type_label().child(title(k)))
                        .children(keys(k).map(|keys| combo(keys, c))),
                )
                .child(
                    div()
                        .type_small()
                        .tabular()
                        .text_color(c.text_muted)
                        .child(summary(k, s)),
                ),
        )
        .children(switch);
    v_flex()
        .rounded(radius::MD)
        .bg(c.hover)
        .px(space::MD)
        .pb(space::XS)
        .child(header)
        .children(body(k, s, on.unwrap_or(true), c, cx))
        .into_any_element()
}

fn title(k: Card) -> &'static str {
    match k {
        Card::Backdrop => "Backdrop",
        Card::Stage => "Stage",
        Card::Particles => "Particles",
        Card::Wave => "Wave",
        Card::Glow => "Glow",
        Card::Halos => "Beat halos",
        Card::Seek => "Seek bar",
        Card::Visualizer => "Visualiser",
        Card::Dissolve => "Cover dissolve",
        Card::Flight => "Cover flight",
    }
}

/// The key that opens what the card dresses: Stage (F), the visualiser
/// (V).
fn keys(k: Card) -> Option<&'static [&'static str]> {
    match k {
        Card::Stage => Some(&["F"]),
        Card::Visualizer => Some(&["V"]),
        _ => None,
    }
}

/// The card's switch, if it has one.
fn switched(k: Card, s: &VisualsConfig) -> Option<bool> {
    match k {
        Card::Backdrop => Some(s.backdrop.on),
        Card::Particles => Some(s.particles.on),
        Card::Wave => Some(s.wave.on),
        Card::Glow => Some(s.glow.on),
        Card::Halos => Some(s.halos.on),
        Card::Dissolve => Some(s.dissolve.on),
        Card::Flight => Some(s.flight.on),
        Card::Stage | Card::Seek | Card::Visualizer => None,
    }
}

fn set_switch(k: Card, s: &mut VisualsConfig, on: bool) {
    match k {
        Card::Backdrop => s.backdrop.on = on,
        Card::Particles => s.particles.on = on,
        Card::Wave => s.wave.on = on,
        Card::Glow => s.glow.on = on,
        Card::Halos => s.halos.on = on,
        Card::Dissolve => s.dissolve.on = on,
        Card::Flight => s.flight.on = on,
        Card::Stage | Card::Seek | Card::Visualizer => {}
    }
}

fn summary(k: Card, s: &VisualsConfig) -> String {
    let off = |on: bool, text: String| if on { text } else { "Off".into() };
    match k {
        Card::Backdrop => off(
            s.backdrop.on,
            "The cover, flowing behind Now Playing and Stage".into(),
        ),
        Card::Stage => if s.stage.backdrop {
            "The backdrop shows behind Stage"
        } else {
            "Stage stays plain"
        }
        .into(),
        Card::Particles => off(
            s.particles.on,
            "Fine sparkles drifting over the backdrop".into(),
        ),
        Card::Wave => off(
            s.wave.on,
            "Soft ribbons of the cover's colours flowing across".into(),
        ),
        Card::Glow => off(
            s.glow.on,
            format!(
                "The cover's colours in the player bar at {:.0}%",
                s.glow.intensity * 100.
            ),
        ),
        Card::Halos => off(
            s.halos.on,
            format!(
                "Rings round play and the cover at {:.0}%",
                s.halos.strength * 100.
            ),
        ),
        Card::Seek => match (s.seek.waveform, s.seek.ridge) {
            (true, true) => "The song's loudness and its most-replayed part".into(),
            (true, false) => "The song's loudness".into(),
            (false, true) => "The most-replayed part".into(),
            (false, false) => "A plain line".into(),
        },
        Card::Visualizer => visualizer_summary(s),
        Card::Dissolve => off(
            s.dissolve.on,
            format!(
                "The old cover burns into the new one in {} ms",
                s.dissolve.ms
            ),
        ),
        Card::Flight => off(
            s.flight.on,
            format!("The cover flies into Now Playing in {} ms", s.flight.ms),
        ),
    }
}

fn visualizer_summary(s: &VisualsConfig) -> String {
    let v = &s.visualizer;
    let mut places = Vec::new();
    if v.now_playing.visualizer() {
        places.push("Now Playing");
    }
    if s.stage.visualizer {
        places.push("Stage");
    }
    if places.is_empty() {
        format!("{}, in the full window (V)", v.style.label())
    } else {
        format!(
            "{} in {} and the full window (V)",
            v.style.label(),
            places.join(" and ")
        )
    }
}

/// The settings inside a card; `on` is its switch.
fn body(
    k: Card,
    s: &VisualsConfig,
    on: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    let sliders = |knobs: &[Knob], cx: &mut Context<MusicApp>| -> Vec<AnyElement> {
        knobs.iter().map(|k| knobs::row(*k, on, c, cx)).collect()
    };
    match k {
        Card::Backdrop => sliders(
            &[
                Knob::Intensity,
                Knob::Blur,
                Knob::Swirl,
                Knob::Bloom,
                Knob::BassPulse,
            ],
            cx,
        ),
        Card::Stage => vec![toggle(
            "visuals-stage-backdrop",
            "Backdrop in Stage",
            "The flowing cover behind Stage",
            s.stage.backdrop,
            true,
            c,
            cx,
            |s, v| s.stage.backdrop = v,
        )],
        Card::Particles => {
            let mut rows = sliders(
                &[
                    Knob::Amount,
                    Knob::Size,
                    Knob::Softness,
                    Knob::Brightness,
                    Knob::Speed,
                    Knob::Direction,
                    Knob::Depth,
                    Knob::Twinkle,
                    Knob::TwinkleSpeed,
                    Knob::Reaction,
                ],
                cx,
            );
            rows.push(colours(s, on, c, cx));
            rows
        }
        Card::Wave => {
            let mut rows = sliders(&[Knob::WaveStrength, Knob::WaveSpeed, Knob::WaveHeight], cx);
            rows.push(ribbons(s, on, c, cx));
            rows
        }
        Card::Glow => sliders(&[Knob::Glow], cx),
        Card::Halos => sliders(&[Knob::Halos], cx),
        Card::Dissolve => sliders(&[Knob::Dissolve], cx),
        Card::Flight => sliders(&[Knob::Flight], cx),
        Card::Seek => vec![
            toggle(
                "visuals-wave",
                "Waveform",
                "The song's loudness under the played part",
                s.seek.waveform,
                true,
                c,
                cx,
                |s, v| s.seek.waveform = v,
            ),
            toggle(
                "visuals-ridge",
                "Most replayed",
                "A ridge over the part people replay most",
                s.seek.ridge,
                true,
                c,
                cx,
                |s, v| s.seek.ridge = v,
            ),
        ],
        Card::Visualizer => super::visualiser::rows(s, c, cx),
    }
}

/// The particles' colour: white, or tinted by the cover or the accent.
fn colours(s: &VisualsConfig, on: bool, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let chips = ParticleColour::ALL.map(|colour| {
        let chip = super::super::choice(
            ("visuals-particle-colour", colour as usize),
            colour.label(),
            s.particles.colour == colour,
            c,
        );
        if on {
            chip.on_click(
                cx.listener(move |_, _, _, cx| change(cx, |s| s.particles.colour = colour)),
            )
        } else {
            chip.opacity(0.5)
        }
    });
    labelled("Colour", super::super::choices(chips))
}

/// How many ribbons the wave has.
fn ribbons(s: &VisualsConfig, on: bool, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let chips = [1u32, 2, 3].map(|n| {
        let chip = super::super::choice(
            ("visuals-ribbons", n as usize),
            format!("{n}"),
            s.wave.ribbons == n,
            c,
        );
        if on {
            chip.on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.wave.ribbons = n)))
        } else {
            chip.opacity(0.5)
        }
    });
    labelled("Ribbons", super::super::choices(chips))
}
