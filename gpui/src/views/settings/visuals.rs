//! Settings → Visuals: the look's preset on top (Off, Calm, Default,
//! Vivid), then one collapsible group per effect, its switch in the
//! header and its sliders and choices inside, and the frame rate cap.
//! Changes show at once (`visuals::config`); sliders save when let go.

mod knobs;

use gpui_kit::assets::IconName;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};
use crate::visuals::config::{
    self, Palette, Placement, Preset, Spacing, Style, Swatch, VisualsConfig,
};
use knobs::{Knob, Knobs};

/// An effect's group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Backdrop,
    Visualizer,
    Glow,
    Halos,
    Seek,
    Dissolve,
    Flight,
    Stage,
}

/// The swatches' size.
const SWATCH: Pixels = px(24.);

pub fn section(
    _app: &MusicApp,
    c: &Colors,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    Knobs::sync(window, cx);
    let saved = config::saved();
    let mut rows = vec![look(&saved, c, cx), presets(&saved, c, cx)];
    if saved.on {
        rows.push(
            v_flex()
                .gap(space::XS)
                .pb(space::SM)
                .children(GROUPS.map(|g| group(g, &saved, c, cx)))
                .into_any_element(),
        );
        rows.push(frame_rate(&saved, c, cx));
    }
    super::section("Visuals", c, rows)
}

const GROUPS: [Group; 8] = [
    Group::Backdrop,
    Group::Visualizer,
    Group::Glow,
    Group::Halos,
    Group::Seek,
    Group::Dissolve,
    Group::Flight,
    Group::Stage,
];

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
        match preset {
            Preset::Off => "No effects: plain backgrounds and no visualiser".into(),
            Preset::Calm => "Slower, softer effects".into(),
            Preset::Default => "The effects as they come".into(),
            Preset::Vivid => "More colour, more motion".into(),
        }
    };
    let reset = (!saved.is_preset()).then(|| {
        widgets::pill_button(
            "visuals-reset",
            format!("Reset to {}", preset.label()),
            None,
            Pill::Secondary,
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
        super::choice(
            ("visuals-fps", fps as usize),
            format!("{fps}"),
            saved.fps == fps,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.fps = fps)))
    });
    v_flex()
        .child(super::row(
            "Frame rate",
            Some("Frames a second at most while effects move. Fewer use less power.".into()),
            div(),
            c,
        ))
        .child(super::choices(chips))
        .into_any_element()
}

/// A group: a header that opens and closes it (name, what it is set to,
/// and its switch), and its settings while open.
fn group(g: Group, saved: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let open = cx
        .try_global::<Knobs>()
        .is_some_and(|k| k.open.borrow().contains(&g));
    let on = switched(g, saved);
    let switch = on.map(|on| {
        widgets::switch(("visuals-switch", g as usize), on, c).on_click(cx.listener(
            move |_, on: &bool, _, cx| {
                let on = *on;
                change(cx, |s| set_switch(g, s, on));
            },
        ))
    });
    let chevron = if open {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    let hover = c.hover;
    let header = h_flex()
        .id(("visuals-group", g as usize))
        .flex_1()
        .min_w_0()
        .py(space::SM)
        .gap(space::MD)
        .cursor_pointer()
        .child(widgets::icon(chevron, size::ICON_SM, c.text_muted))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().type_label().child(title(g)))
                .child(
                    div()
                        .type_small()
                        .tabular()
                        .truncate()
                        .text_color(c.text_muted)
                        .child(summary(g, saved)),
                ),
        )
        .on_click(cx.listener(move |_, _, _, cx| {
            if let Some(knobs) = cx.try_global::<Knobs>() {
                let mut open = knobs.open.borrow_mut();
                if !open.remove(&g) {
                    open.insert(g);
                }
            }
            cx.notify();
        }));
    v_flex()
        .rounded(radius::MD)
        .bg(c.hover)
        .px(space::MD)
        .child(
            h_flex()
                .gap(space::MD)
                .rounded(radius::MD)
                .hover(move |s| s.bg(hover))
                .child(header)
                .children(switch),
        )
        .when(open, |d| {
            d.child(
                v_flex()
                    .pl(size::ICON_SM + space::MD)
                    .pb(space::SM)
                    .children(body(g, saved, on.unwrap_or(true), c, cx)),
            )
        })
        .into_any_element()
}

fn title(g: Group) -> &'static str {
    match g {
        Group::Backdrop => "Backdrop",
        Group::Visualizer => "Visualiser",
        Group::Glow => "Player bar glow",
        Group::Halos => "Beat halos",
        Group::Seek => "Seek bar",
        Group::Dissolve => "Cover dissolve",
        Group::Flight => "Cover flight",
        Group::Stage => "Stage",
    }
}

/// The group's switch, for groups that have one.
fn switched(g: Group, s: &VisualsConfig) -> Option<bool> {
    match g {
        Group::Backdrop => Some(s.backdrop.on),
        Group::Glow => Some(s.glow.on),
        Group::Halos => Some(s.halos.on),
        Group::Dissolve => Some(s.dissolve.on),
        Group::Flight => Some(s.flight.on),
        Group::Visualizer | Group::Seek | Group::Stage => None,
    }
}

fn set_switch(g: Group, s: &mut VisualsConfig, on: bool) {
    match g {
        Group::Backdrop => s.backdrop.on = on,
        Group::Glow => s.glow.on = on,
        Group::Halos => s.halos.on = on,
        Group::Dissolve => s.dissolve.on = on,
        Group::Flight => s.flight.on = on,
        Group::Visualizer | Group::Seek | Group::Stage => {}
    }
}

/// One line under the group's name: what it does or how it is set.
fn summary(g: Group, s: &VisualsConfig) -> String {
    let off = |on: bool, text: String| if on { text } else { "Off".into() };
    match g {
        Group::Backdrop => off(
            s.backdrop.on,
            "The cover, flowing behind Now Playing and Stage".into(),
        ),
        Group::Visualizer => {
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
        Group::Glow => off(
            s.glow.on,
            format!("The cover's colours at {:.0}%", s.glow.intensity * 100.),
        ),
        Group::Halos => off(
            s.halos.on,
            format!(
                "Rings round play and the cover at {:.0}%",
                s.halos.strength * 100.
            ),
        ),
        Group::Seek => match (s.seek.waveform, s.seek.ridge) {
            (true, true) => "The song's loudness and its most-replayed part".into(),
            (true, false) => "The song's loudness".into(),
            (false, true) => "The most-replayed part".into(),
            (false, false) => "A plain line".into(),
        },
        Group::Dissolve => off(
            s.dissolve.on,
            format!(
                "The old cover burns into the new one in {} ms",
                s.dissolve.ms
            ),
        ),
        Group::Flight => off(
            s.flight.on,
            format!("The cover flies into Now Playing in {} ms", s.flight.ms),
        ),
        Group::Stage => match (s.stage.backdrop, s.stage.visualizer) {
            (true, true) => "Backdrop and visualiser".into(),
            (true, false) => "Backdrop".into(),
            (false, true) => "Visualiser".into(),
            (false, false) => "Plain".into(),
        },
    }
}

/// The settings inside an open group; `on` is its switch.
fn body(
    g: Group,
    s: &VisualsConfig,
    on: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    match g {
        Group::Backdrop => {
            let mut rows: Vec<AnyElement> = [Knob::Intensity, Knob::Blur, Knob::Swirl, Knob::Bloom]
                .map(|k| knobs::row(k, on, c, cx))
                .into();
            rows.push(knobs::row(Knob::BassPulse, on, c, cx));
            rows.push(toggle(
                "visuals-motes",
                "Motes",
                "Specks of light drifting up, flaring on the beat",
                s.backdrop.motes,
                on,
                c,
                cx,
                |s, v| s.backdrop.motes = v,
            ));
            rows.push(knobs::row(Knob::Motes, on && s.backdrop.motes, c, cx));
            rows.push(knobs::row(Knob::MoteSize, on && s.backdrop.motes, c, cx));
            rows
        }
        Group::Glow => vec![knobs::row(Knob::Glow, on, c, cx)],
        Group::Halos => vec![knobs::row(Knob::Halos, on, c, cx)],
        Group::Dissolve => vec![knobs::row(Knob::Dissolve, on, c, cx)],
        Group::Flight => vec![knobs::row(Knob::Flight, on, c, cx)],
        Group::Seek => vec![
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
        Group::Stage => vec![
            toggle(
                "visuals-stage-backdrop",
                "Backdrop",
                "The flowing cover behind Stage",
                s.stage.backdrop,
                true,
                c,
                cx,
                |s, v| s.stage.backdrop = v,
            ),
            toggle(
                "visuals-stage-visualizer",
                "Visualiser",
                "Moves with the music in Stage",
                s.stage.visualizer,
                true,
                c,
                cx,
                |s, v| s.stage.visualizer = v,
            ),
        ],
        Group::Visualizer => visualizer(s, c, cx),
    }
}

/// A switch row inside a group.
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
    let control = widgets::switch(id, value, c)
        .disabled(!enabled)
        .on_click(cx.listener(move |_, on: &bool, _, cx| {
            let on = *on;
            change(cx, |s| set(s, on));
        }));
    super::row(label, Some(detail.into()), control, c)
}

/// A label over a row of choices, inside a group.
fn labelled(label: &'static str, chips: AnyElement) -> AnyElement {
    v_flex()
        .gap(space::XS)
        .pt(space::SM)
        .child(div().type_body().child(label))
        .child(chips)
        .into_any_element()
}

fn visualizer(s: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let v = &s.visualizer;
    let styles = super::choices(Style::ALL.map(|style| {
        super::choice(
            ("visuals-style", style as usize),
            style.label(),
            v.style == style,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.style = style)))
    }));
    let places = super::choices(Placement::ALL.map(|p| {
        super::choice(
            ("visuals-place", p as usize),
            p.label(),
            v.now_playing == p,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.now_playing = p)))
    }));
    let spacing = super::choices(
        [(Spacing::Log, "Octaves"), (Spacing::Linear, "Linear")].map(|(sp, label)| {
            super::choice(("visuals-spacing", sp as usize), label, v.spacing == sp, c)
                .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.spacing = sp)))
        }),
    );
    let palettes = super::choices(Palette::ALL.map(|p| {
        super::choice(
            ("visuals-palette", p as usize),
            p.label(),
            v.palette == p,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.palette = p)))
    }));
    let mut rows = vec![
        labelled("Style", styles),
        labelled("In Now Playing", places),
        knobs::row(Knob::Bars, true, c, cx),
        knobs::row(Knob::Sensitivity, true, c, cx),
        knobs::row(Knob::Smoothing, true, c, cx),
        knobs::row(Knob::Decay, true, c, cx),
        knobs::row(Knob::Frequencies, true, c, cx),
        labelled("Spacing", spacing),
        toggle(
            "visuals-peaks",
            "Peak caps",
            "A cap over each bar that falls slowly",
            v.peaks,
            true,
            c,
            cx,
            |s, on| s.visualizer.peaks = on,
        ),
        knobs::row(Knob::PeakFall, v.peaks, c, cx),
        labelled("Colours", palettes),
    ];
    if v.palette == Palette::Custom {
        rows.push(swatches(0, "From", v.custom[0], c, cx));
        rows.push(swatches(1, "To", v.custom[1], c, cx));
    }
    rows.push(knobs::row(Knob::Opacity, true, c, cx));
    rows.push(knobs::row(Knob::VisGlow, true, c, cx));
    rows.push(open_button(c, cx));
    rows
}

/// A row of the custom gradient's colours to pick one stop from.
fn swatches(
    stop: usize,
    label: &'static str,
    chosen: Swatch,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let ring = c.text;
    let dots = Swatch::ALL.map(|swatch| {
        let [r, g, b] = swatch.rgb();
        let fill: Hsla = Rgba { r, g, b, a: 1. }.into();
        let picked = swatch == chosen;
        div()
            .id(SharedString::from(format!(
                "visuals-swatch-{stop}-{}",
                swatch.label()
            )))
            .size(SWATCH)
            .flex_none()
            .rounded(radius::FULL)
            .p(px(3.))
            .border_2()
            .border_color(if picked { ring } else { transparent_black() })
            .cursor_pointer()
            .child(div().size_full().rounded(radius::FULL).bg(fill))
            .tooltip(widgets::tooltip(swatch.label()))
            .on_click(
                cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.custom[stop] = swatch)),
            )
    });
    h_flex()
        .py(space::XS)
        .gap(space::MD)
        .child(
            div()
                .w(px(40.))
                .type_small()
                .text_color(c.text_muted)
                .child(label),
        )
        .child(h_flex().flex_wrap().gap(space::XS).children(dots))
        .into_any_element()
}

/// Closes Settings and opens the full-window visualiser.
fn open_button(c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    h_flex()
        .pt(space::SM)
        .child(
            widgets::pill_button(
                "visuals-open",
                "Open visualiser",
                Some(widgets::icon(IconName::AudioLines, size::ICON_SM, c.text)),
                Pill::Secondary,
                c,
            )
            .tooltip(widgets::tooltip("Fills the window (V)"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_settings(false, window, cx);
                if !this.extras.visualizer {
                    this.toggle_visualizer(window, cx);
                }
            })),
        )
        .into_any_element()
}
