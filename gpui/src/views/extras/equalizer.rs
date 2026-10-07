//! The equalizer panel (E): presets as chips, the ten bands as one curve to
//! drag, a switch that bypasses it, and the playback settings that shape
//! the sound between songs (loudness levelling, smooth mixes) until they
//! move into Settings.

use gpui_kit::assets::IconName;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::equalizer::{Equalizer, Preset};
use ytfast::model::Mixes;

use super::super::widgets;
use super::sleep::{appear, floating};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};

const WIDTH: Pixels = px(560.);

pub fn panel(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let eq = app.equalizer();
    let panel = floating("equalizer-panel", app, c, cx)
        .w(WIDTH)
        .p(space::XL)
        .gap(space::LG)
        .child(header(&eq, c, cx))
        .child(presets(&eq, c, cx))
        .child(super::eq_graph::graph(app, &eq, c, cx))
        .child(
            div()
                .type_caption()
                .tabular()
                .text_color(c.text_faint)
                .child(preamp_line(&eq)),
        )
        .child(playback(app, c, cx));
    appear(panel, "equalizer-panel")
}

fn header(eq: &Equalizer, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let on = eq.enabled;
    let bypass = eq.clone();
    h_flex()
        .gap(space::MD)
        .child(div().flex_1().type_heading().child("Equalizer"))
        .child(
            Switch::new("equalizer-on")
                .checked(on)
                .color(c.signal)
                .tooltip(if on {
                    "Turn the equalizer off"
                } else {
                    "Turn the equalizer on"
                })
                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                    let eq = Equalizer {
                        enabled: *checked,
                        ..bypass.clone()
                    };
                    this.set_equalizer(eq, cx);
                })),
        )
        .child(
            widgets::icon_button(
                "close-equalizer",
                widgets::icon(IconName::X, size::ICON, c.text_muted),
                c,
            )
            .tooltip(widgets::tooltip("Close"))
            .on_click(cx.listener(|this, _, window, cx| this.close_panels(window, cx))),
        )
}

/// The presets as chips; the chosen one is lifted in the primary colour.
fn presets(eq: &Equalizer, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let chosen = eq.enabled.then_some(eq.preset);
    let mut shown: Vec<Preset> = Preset::ALL.to_vec();
    if chosen == Some(Preset::Custom) {
        shown.push(Preset::Custom);
    }
    h_flex()
        .flex_wrap()
        .gap(space::SM)
        .children(shown.into_iter().enumerate().map(|(i, preset)| {
            let on = chosen == Some(preset);
            let (bg, fg, hover) = if on {
                (c.primary, c.primary_foreground, c.primary_hover)
            } else {
                (c.raised, c.text, c.raised.blend(c.hover))
            };
            h_flex()
                .id(("eq-preset", i))
                .h(px(32.))
                .px(space::MD)
                .rounded(radius::FULL)
                .bg(bg)
                .text_color(fg)
                .type_label()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(preset.label())
                .when(preset != Preset::Custom, |chip| {
                    chip.on_click(cx.listener(move |this, _, _, cx| this.eq_preset(preset, cx)))
                })
        }))
}

/// The headroom the equalizer takes so boosted bands don't clip.
fn preamp_line(eq: &Equalizer) -> String {
    let preamp = if eq.active() { eq.preamp() } else { 0.0 };
    let preamp = if preamp.abs() < 0.05 {
        "0 dB".to_string()
    } else {
        format!("−{:.1} dB", preamp.abs())
    };
    format!("Preamp {preamp}, set so boosts don't clip. Double-click a band to reset it.")
}

/// Loudness levelling and smooth mixes, on a raised group under the bands.
fn playback(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let playback = &app.player.playback;
    let normalize = playback.normalize;
    let mixes = playback.mixes;
    let gain = playback
        .gain
        .filter(|_| normalize)
        .map(|g| format!(" This song: {g:+.1} dB."))
        .unwrap_or_default();
    v_flex()
        .mt(space::XS)
        .p(space::XS)
        .rounded(radius::MD + space::XS)
        .bg(c.raised)
        .child(setting_row(
            "Loudness levelling",
            format!("Plays every song at a similar volume.{gain}"),
            Switch::new("normalize")
                .checked(normalize)
                .color(c.signal)
                .on_click(cx.listener(|this, on: &bool, _, cx| this.set_normalize(*on, cx)))
                .into_any_element(),
            c,
        ))
        .child(setting_row(
            "Smooth mixes",
            "Crossfades songs on radios, mixes and autoplay. Albums stay gapless.".into(),
            h_flex()
                .gap(space::MD)
                .when(mixes.on, |s| s.child(mix_length(mixes, c, cx)))
                .child(
                    Switch::new("mixes")
                        .checked(mixes.on)
                        .color(c.signal)
                        .on_click(cx.listener(move |this, on: &bool, _, cx| {
                            this.set_mixes(Mixes { on: *on, ..mixes }, cx)
                        })),
                )
                .into_any_element(),
            c,
        ))
}

fn setting_row(title: &'static str, detail: String, control: AnyElement, c: &Colors) -> Div {
    h_flex()
        .px(space::MD)
        .py(space::SM)
        .gap(space::LG)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().type_label().child(title))
                .child(div().type_small().text_color(c.text_muted).child(detail)),
        )
        .child(control)
}

/// The crossfade's length: − 6 s +.
fn mix_length(mixes: Mixes, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let step = |id: &'static str, icon: IconName, by: i8, enabled: bool| {
        let color = if enabled {
            c.text_muted
        } else {
            c.text_faint.opacity(0.5)
        };
        h_flex()
            .id(id)
            .size(px(28.))
            .justify_center()
            .rounded(radius::FULL)
            .child(widgets::icon(icon, size::ICON_SM, color))
            .when(enabled, |s| {
                s.cursor_pointer()
                    .hover(|s| s.bg(c.hover))
                    .active(|s| s.bg(c.pressed))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let seconds = mixes.seconds.saturating_add_signed(by);
                        this.set_mixes(Mixes { seconds, ..mixes }, cx)
                    }))
            })
    };
    h_flex()
        .gap(space::XXS)
        .child(step(
            "mixes-shorter",
            IconName::Minus,
            -1,
            mixes.seconds > Mixes::SHORTEST,
        ))
        .child(
            div()
                .w(px(32.))
                .text_center()
                .type_caption()
                .tabular()
                .child(format!("{} s", mixes.seconds)),
        )
        .child(step(
            "mixes-longer",
            IconName::Plus,
            1,
            mixes.seconds < Mixes::LONGEST,
        ))
}
