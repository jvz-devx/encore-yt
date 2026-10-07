//! The Visualiser tab: style, where it shows, how the bars move and their
//! colours, and a button to open the full-window visualiser.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::super::super::widgets::{self, Pill};
use super::super::{choice, choices};
use super::knobs::{self, Knob};
use super::{change, labelled, toggle};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};
use crate::visuals::config::{Palette, Placement, Spacing, Style, Swatch, VisualsConfig};

/// The swatches' size.
const SWATCH: Pixels = px(24.);

pub fn rows(s: &VisualsConfig, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let v = &s.visualizer;
    let styles = choices(Style::ALL.map(|style| {
        choice(
            ("visuals-style", style as usize),
            style.label(),
            v.style == style,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.style = style)))
    }));
    let places = choices(Placement::ALL.map(|p| {
        choice(
            ("visuals-place", p as usize),
            p.label(),
            v.now_playing == p,
            c,
        )
        .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.now_playing = p)))
    }));
    let spacing = choices(
        [(Spacing::Log, "Octaves"), (Spacing::Linear, "Linear")].map(|(sp, label)| {
            choice(("visuals-spacing", sp as usize), label, v.spacing == sp, c)
                .on_click(cx.listener(move |_, _, _, cx| change(cx, |s| s.visualizer.spacing = sp)))
        }),
    );
    let palettes = choices(Palette::ALL.map(|p| {
        choice(
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
        toggle(
            "visuals-stage-visualizer",
            "In Stage",
            "Moves with the music under Stage's cover and lyrics",
            s.stage.visualizer,
            true,
            c,
            cx,
            |s, v| s.stage.visualizer = v,
        ),
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
            .child(
                div()
                    .size_full()
                    .rounded(radius::FULL)
                    .bg(fill)
                    .border_1()
                    .border_color(c.outline),
            )
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
    let button = super::super::focusable(
        widgets::pill_button(
            "visuals-open",
            "Open visualiser",
            Some(widgets::icon(IconName::AudioLines, size::ICON_SM, c.text)),
            Pill::Tonal,
            c,
        ),
        c,
    )
    .tooltip(widgets::tooltip("Fills the window"))
    .on_click(cx.listener(|this, _, window, cx| {
        this.open_settings(false, window, cx);
        if !this.extras.visualizer {
            this.toggle_visualizer(window, cx);
        }
    }));
    h_flex()
        .pt(space::SM)
        .pb(space::SM)
        .child(super::super::keyed(button, &["V"], c))
        .into_any_element()
}
