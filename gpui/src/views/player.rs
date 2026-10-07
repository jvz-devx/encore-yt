//! The player bar: the song, transport controls, position and volume.

pub(super) mod links;
mod side;

use gpui_kit::assets::IconName;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::Repeat;

use super::clock;
use super::widgets;
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::playback::SEEK_SCALE;
use crate::theme::{self, Colors, Type, radius, size, space};
use crate::visuals::{self, Slot};

/// The song and the volume take the same width, so the transport is centred
/// on the window.
const SIDE: Pixels = px(300.);
/// The seek bar's widest.
const SEEK_MAX: Pixels = px(600.);

pub fn player_bar(app: &MusicApp, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    // The effects layer paints the bar's background and seek bar (M9).
    let painted = visuals::paints_bar(cx);
    h_flex()
        .relative()
        .h(size::PLAYER_BAR)
        .flex_none()
        .px(space::LG)
        .gap(space::XL)
        .when(!painted, |bar| bar.bg(c.base))
        .child(visuals::slot(Slot::Bar))
        .child(song(app, &c, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .items_center()
                .child(transport(app, &c, cx))
                .child(position(app, &c, cx)),
        )
        .child(side::side(app, SIDE, &c, cx))
}

/// The cover and the song's title and artists. A click on it (outside the
/// links) opens Now Playing, and closes it again.
fn song(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let track = app.player.current();
    let open = app.player.now_playing;
    let text = match track {
        None => v_flex().min_w_0().flex_1().child(
            div()
                .type_small()
                .text_color(c.text_faint)
                .child("Nothing playing"),
        ),
        Some(track) => v_flex()
            .min_w_0()
            .flex_1()
            .child(div().truncate().type_label().child(track.title.clone()))
            .child(
                links::track_links("bar", track, app, c, cx)
                    .type_small()
                    .text_color(c.text_muted),
            ),
    };
    let like = track.and_then(|t| super::account::like_button(app, t, cx));
    let url = track.and_then(|t| t.thumbnail.clone());
    visuals::set_bar_cover(
        url.as_deref()
            .map(|u| super::page::covers::sized(u, size::PLAYER_COVER).into()),
        cx,
    );
    h_flex()
        .w(SIDE)
        .flex_none()
        .gap(space::MD)
        .child(
            h_flex()
                .id("song")
                .flex_1()
                .min_w_0()
                .gap(space::MD)
                .rounded(radius::SM)
                .when(track.is_some(), |s| s.cursor_pointer())
                .child(
                    widgets::cover(url.map(Into::into), size::PLAYER_COVER, false, c)
                        .child(visuals::slot(Slot::BarCover))
                        .when(open, |cover| cover.child(collapse_badge(c))),
                )
                .child(text)
                .on_click(cx.listener(move |this, _, _, cx| this.show_now_playing(!open, cx))),
        )
        .children(like)
}

/// While Now Playing is open, the bar's cover shows how to close it.
fn collapse_badge(c: &Colors) -> impl IntoElement {
    h_flex()
        .absolute()
        .inset_0()
        .justify_center()
        .rounded(radius::SM)
        .bg(c.scrim)
        .child(widgets::icon(IconName::ChevronDown, size::ICON, c.on_media))
}

fn transport(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let playback = &app.player.playback;
    // Tooltips are built when they appear, so they name the control rather
    // than a state that may change under them.
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
            .tooltip(widgets::tooltip("Shuffle"))
            .on_click(cx.listener(|this, _, _, _| this.send(Command::ToggleShuffle))),
        )
        .child(
            widgets::icon_button(
                "previous",
                widgets::glyph(Glyph::SkipBack, px(20.), c.text),
                c,
            )
            .tooltip(widgets::tooltip("Previous"))
            .on_click(cx.listener(|this, _, _, _| this.send(Command::Previous))),
        )
        .child(
            play_pause(playback.playing, playback.loading, c)
                .tooltip(widgets::tooltip("Play or pause"))
                .on_click(cx.listener(|this, _, _, _| this.send(Command::TogglePause))),
        )
        .child(
            widgets::icon_button(
                "next",
                widgets::glyph(Glyph::SkipForward, px(20.), c.text),
                c,
            )
            .tooltip(widgets::tooltip("Next"))
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
            .tooltip(widgets::tooltip("Repeat"))
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
        .relative()
        .mx(space::XS)
        .size(size::PLAY_BUTTON)
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.primary)
        .when(!playing && !loading, |s| s.pl(px(2.)))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.9))
        .child(visuals::slot(Slot::Play))
        .child(content)
}

/// Elapsed, the seek bar and the length. While the handle is held, the
/// elapsed time follows the handle.
fn position(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let player = &app.player;
    let duration = player.playback.duration;
    let elapsed = if player.seeking {
        f64::from(player.seek.read(cx).value().start() / SEEK_SCALE) * duration
    } else {
        player.position()
    };
    let time = |t: String| {
        div()
            .w(px(40.))
            .flex_none()
            .type_caption()
            .tabular()
            .text_color(c.text_faint)
            .child(t)
    };
    let known = duration > 0.0;
    // Under the slider, the effects layer draws the seek bar (with the
    // song's waveform); the ridge draws over both.
    let (fill, thumb) = if visuals::paints_bar(cx) {
        (transparent_black(), transparent_black())
    } else {
        (c.signal, c.text)
    };
    h_flex()
        .w_full()
        .max_w(SEEK_MAX)
        .gap(space::SM)
        .child(
            time(if known || elapsed > 0.0 {
                clock(elapsed)
            } else {
                String::new()
            })
            .text_right(),
        )
        .child(
            div()
                .flex_1()
                .relative()
                .child(visuals::slot(Slot::Seek))
                .children(super::extras::ridge(app, c, cx))
                .child(
                    Slider::new(&player.seek)
                        .disabled(!known)
                        .bg(fill)
                        .text_color(thumb),
                ),
        )
        .child(time(if known {
            clock(duration)
        } else {
            String::new()
        }))
}
