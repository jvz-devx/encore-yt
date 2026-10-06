//! The player bar: the song, transport controls, position and volume.

use gpui_kit::assets::IconName;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::Repeat;

use super::widgets;
use super::{clock, runs_text};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{self, Colors, Type, radius, size, space};

/// The song and the volume take the same width, so the transport is centred
/// on the window.
const SIDE: Pixels = px(300.);

pub fn player_bar(app: &MusicApp, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    h_flex()
        .h(size::PLAYER_BAR)
        .flex_none()
        .px(space::LG)
        .gap(space::XL)
        .bg(c.base)
        .child(song(app, &c))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .items_center()
                .child(transport(app, &c, cx))
                .child(position(app, &c)),
        )
        .child(volume(app, &c))
}

fn song(app: &MusicApp, c: &Colors) -> impl IntoElement {
    let track = app.player.current();
    let title = track.map(|t| t.title.clone()).unwrap_or_default();
    let artists = track.map(|t| runs_text(&t.artists)).unwrap_or_default();
    h_flex()
        .w(SIDE)
        .flex_none()
        .gap(space::MD)
        .child(widgets::cover(
            track.and_then(|t| t.thumbnail.clone()).map(Into::into),
            size::PLAYER_COVER,
            false,
            c,
        ))
        .child(
            v_flex()
                .min_w_0()
                .flex_1()
                .when(track.is_none(), |s| {
                    s.child(
                        div()
                            .type_small()
                            .text_color(c.text_faint)
                            .child("Nothing playing"),
                    )
                })
                .when(track.is_some(), |s| {
                    s.child(div().truncate().type_label().child(title))
                        .child(widgets::muted_line(artists, c))
                }),
        )
}

fn transport(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let playback = &app.player.playback;
    let repeat_icon = if playback.repeat == Repeat::One {
        IconName::Repeat1
    } else {
        IconName::Repeat
    };
    h_flex()
        .gap(space::SM)
        .child(
            widgets::icon_button(
                "shuffle",
                widgets::icon(
                    IconName::Shuffle,
                    size::ICON,
                    widgets::toggle_color(playback.shuffle, c),
                ),
                c,
            )
            .on_click(cx.listener(|this, _, _, _| this.send(Command::ToggleShuffle))),
        )
        .child(
            widgets::icon_button(
                "previous",
                widgets::glyph(Glyph::SkipBack, px(20.), c.text),
                c,
            )
            .on_click(cx.listener(|this, _, _, _| this.send(Command::Previous))),
        )
        .child(
            play_pause(playback.playing, playback.loading, c)
                .on_click(cx.listener(|this, _, _, _| this.send(Command::TogglePause))),
        )
        .child(
            widgets::icon_button(
                "next",
                widgets::glyph(Glyph::SkipForward, px(20.), c.text),
                c,
            )
            .on_click(cx.listener(|this, _, _, _| this.send(Command::Next))),
        )
        .child(
            widgets::icon_button(
                "repeat",
                widgets::icon(
                    repeat_icon,
                    size::ICON,
                    widgets::toggle_color(playback.repeat != Repeat::Off, c),
                ),
                c,
            )
            .on_click(cx.listener(|this, _, _, _| this.send(Command::CycleRepeat))),
        )
}

/// The prominent button: a disc in the primary colour.
fn play_pause(playing: bool, loading: bool, c: &Colors) -> Stateful<Div> {
    let fg = c.primary_foreground;
    let content = if loading {
        Spinner::new().color(fg).into_any_element()
    } else if playing {
        widgets::glyph(Glyph::Pause, size::ICON, fg).into_any_element()
    } else {
        widgets::glyph(Glyph::Play, size::ICON, fg).into_any_element()
    };
    let hover = c.primary_hover;
    h_flex()
        .id("play")
        .mx(space::XS)
        .size(size::PLAY_BUTTON)
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.primary)
        .when(!playing && !loading, |s| s.pl(px(2.)))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.9))
        .child(content)
}

fn position(app: &MusicApp, c: &Colors) -> impl IntoElement {
    let time = |t: String| {
        div()
            .w(px(40.))
            .flex_none()
            .type_caption()
            .tabular()
            .text_color(c.text_faint)
            .child(t)
    };
    h_flex()
        .w_full()
        .max_w(px(600.))
        .gap(space::SM)
        .child(time(clock(app.player.position())).text_right())
        .child(
            div().flex_1().child(
                Slider::new(&app.player.seek)
                    .bg(c.signal)
                    .text_color(c.text),
            ),
        )
        .child(time(clock(app.player.playback.duration)))
}

fn volume(app: &MusicApp, c: &Colors) -> impl IntoElement {
    let level = app.player.playback.volume;
    let icon = if level <= 0.0 {
        IconName::VolumeX
    } else if level < 50.0 {
        IconName::Volume1
    } else {
        IconName::Volume2
    };
    h_flex()
        .w(SIDE)
        .flex_none()
        .justify_end()
        .gap(space::SM)
        .child(widgets::icon(icon, size::ICON, c.text_muted))
        .child(
            div().w(px(112.)).child(
                Slider::new(&app.player.volume)
                    .bg(c.text_muted)
                    .text_color(c.text),
            ),
        )
}
