//! Settings → Visuals, a self-contained view in tabs: General (the look's
//! preset, effects on or off, the frame rate), Backdrop, Particles, Player
//! bar, Visualiser, 3D (the 3D scenes) and Transitions. Each tab shows a card per effect (its
//! switch, sliders and choices) and a Reset once it differs from its
//! preset. Changes show at once (`visuals::config`); sliders save when let
//! go. The Settings frame draws the tabs (`Category::tabs`, in [`Tab`]'s
//! order) and [`view`] the tab chosen there.

mod cards;
mod knobs;
mod visualiser;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::settings::Category;
use crate::theme::{Colors, Type, size, space};
use crate::visuals::config::{self, Preset, VisualsConfig};
use cards::Card;
use knobs::Knobs;

/// A tab of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    General,
    Backdrop,
    Particles,
    PlayerBar,
    Visualiser,
    Scenes,
    Transitions,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::General,
        Tab::Backdrop,
        Tab::Particles,
        Tab::PlayerBar,
        Tab::Visualiser,
        Tab::Scenes,
        Tab::Transitions,
    ];

    fn cards(self) -> &'static [Card] {
        match self {
            Tab::General => &[],
            Tab::Backdrop => &[Card::Backdrop, Card::Stage],
            Tab::Particles => &[Card::Particles, Card::Wave],
            Tab::PlayerBar => &[Card::Glow, Card::Halos, Card::Seek],
            Tab::Visualiser => &[Card::Visualizer],
            Tab::Scenes => &[Card::Scenes, Card::Scrim],
            Tab::Transitions => &[Card::Dissolve, Card::Flight],
        }
    }
}

/// Whether the frame shows the tabs: with every effect off only General
/// has anything to set.
pub fn tabs_shown() -> bool {
    config::saved().on
}

/// The tab chosen in the frame.
pub fn view(
    app: &MusicApp,
    c: &Colors,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    Knobs::sync(window, cx);
    let saved = config::saved();
    let chosen = app.settings.tab(Category::Visuals);
    let tab = match Tab::ALL.get(chosen) {
        Some(tab) if saved.on => *tab,
        _ => Tab::General,
    };
    let rows: Vec<AnyElement> = if tab == Tab::General {
        vec![super::section("", c, general(&saved, c, cx))]
    } else {
        let mut rows = vec![tab_reset(tab, &saved, c, cx)];
        rows.extend(tab.cards().iter().map(|k| cards::card(*k, &saved, c, cx)));
        rows
    };
    v_flex()
        .gap(space::SM)
        .pb(space::SM)
        .children(rows)
        .into_any_element()
}

/// General: the look's preset, effects on or off, and the frame rate.
fn general(saved: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let mut rows = vec![look(saved, c, cx), presets(saved, c, cx)];
    rows.push(toggle(
        "visuals-on",
        "Effects",
        if saved.on {
            "Backdrops, glows and the visualiser move with the music"
        } else {
            "Plain backgrounds and no visualiser"
        },
        saved.on,
        true,
        c,
        cx,
        |s, on| s.on = on,
    ));
    if saved.on {
        rows.push(frame_rate(saved, c, cx));
    }
    rows
}

/// Changes the settings, in effect at once and saved.
fn change(cx: &mut Context<MusicApp>, edit: impl FnOnce(&mut VisualsConfig)) {
    let mut saved = config::saved();
    edit(&mut saved);
    config::set(saved, true);
    cx.notify();
}

/// What the look is now, and the way back to its preset once changed.
fn look(saved: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let preset = saved.preset;
    let detail = if !saved.is_preset() {
        format!("Your own, from {}", preset.label())
    } else {
        preset.summary().into()
    };
    let reset = (!saved.is_preset()).then(|| {
        super::focusable(
            widgets::pill_button(
                "visuals-reset",
                format!("Reset to {}", preset.label()),
                None,
                Pill::Tonal,
                c,
            ),
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            change(cx, |s| *s = s.with_preset(preset));
        }))
    });
    super::row("Look", Some(detail.into()), div().children(reset), c)
}

fn presets(saved: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let exact = saved.is_preset();
    super::choices(Preset::ALL.map(|preset| {
        let chosen = saved.preset == preset && exact;
        super::choice(
            ("visuals-preset", preset as usize),
            preset.label(),
            chosen,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| *s = s.with_preset(preset))))
    }))
}

fn frame_rate(saved: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let chips = config::FPS.map(|fps| {
        let label = if fps == config::DISPLAY_FPS {
            "Match display".to_string()
        } else {
            format!("{fps} fps")
        };
        super::choice(("visuals-fps", fps as usize), label, saved.fps == fps, c)
            .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.fps = fps)))
    });
    v_flex()
        .child(super::row(
            "Frame rate",
            Some(
                "How smoothly the effects move. Higher looks more fluid and uses more power."
                    .into(),
            ),
            div(),
            c,
        ))
        .child(super::choices(chips))
        .into_any_element()
}

/// The tab's own settings as its preset has them, with the rest as they
/// are.
fn reset(tab: Tab, s: &VisualsConfig) -> VisualsConfig {
    let r = VisualsConfig::default().with_preset(s.preset);
    let mut out = s.clone();
    match tab {
        Tab::General => out = s.with_preset(s.preset),
        Tab::Backdrop => {
            out.backdrop = r.backdrop;
            out.stage.backdrop = r.stage.backdrop;
        }
        Tab::Particles => {
            out.particles = r.particles;
            out.wave = r.wave;
        }
        Tab::PlayerBar => {
            out.glow = r.glow;
            out.halos = r.halos;
            out.seek = r.seek;
        }
        Tab::Visualiser => {
            out.visualizer = r.visualizer;
            out.stage.visualizer = r.stage.visualizer;
        }
        Tab::Scenes => out.scenes = r.scenes,
        Tab::Transitions => {
            out.dissolve = r.dissolve;
            out.flight = r.flight;
        }
    }
    out
}

/// A line naming the tab's preset, with Reset once the tab differs from it.
fn tab_reset(tab: Tab, s: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let back = reset(tab, s);
    let changed = back != *s;
    let detail = if changed {
        format!("Changed from {}", s.preset.label())
    } else {
        format!("As {} sets it", s.preset.label())
    };
    let button = changed.then(|| {
        super::focusable(
            widgets::pill_button(
                ("visuals-tab-reset", tab as usize),
                "Reset",
                None,
                Pill::Secondary,
                c,
            ),
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| *s = reset(tab, s))))
    });
    h_flex()
        .min_h(size::CHIP)
        .gap(space::MD)
        .child(
            div()
                .flex_1()
                .type_small()
                .text_color(c.text_muted)
                .child(detail),
        )
        .children(button)
        .into_any_element()
}

/// A switch row inside a card.
#[allow(clippy::too_many_arguments)]
fn toggle(
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    value: bool,
    enabled: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
    set: fn(&mut VisualsConfig, bool),
) -> AnyElement {
    use gpui_kit::component::Disableable as _;
    let control = widgets::switch(id, value, c)
        .disabled(!enabled)
        .on_click(cx.listener(move |_, on: &bool, _, cx| {
            let on = *on;
            change(cx, |s| set(s, on));
        }));
    super::row(label, Some(detail.into()), control, c)
}

/// A label over a row of choices, inside a card.
fn labelled(label: &'static str, chips: AnyElement) -> AnyElement {
    v_flex()
        .gap(space::XS)
        .pt(space::SM)
        .child(div().type_body().child(label))
        .child(chips)
        .into_any_element()
}
