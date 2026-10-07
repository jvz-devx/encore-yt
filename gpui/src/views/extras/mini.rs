//! The mini player's window: the cover, the song, the transport, and its
//! progress as a thin line along the bottom edge. The expand button brings
//! back the full window.

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::desktop::Request;

use super::super::{runs_text, widgets};
use crate::assets::Glyph;
use crate::extras::MiniPlayer;
use crate::theme::{self, Colors, Type, radius, size, space};

pub const SIZE: Size<Pixels> = Size {
    width: px(420.),
    height: px(120.),
};
pub const MIN_SIZE: Size<Pixels> = Size {
    width: px(340.),
    height: px(112.),
};
const COVER: Pixels = px(88.);
const PROGRESS: Pixels = px(3.);

pub fn render(
    mini: &mut MiniPlayer,
    _window: &mut Window,
    cx: &mut Context<MiniPlayer>,
) -> impl IntoElement {
    let c = theme::colors(cx);
    let app = mini.app.read(cx);
    let track = app.player.current().cloned();
    let playback = &app.player.playback;
    let (playing, loading) = (playback.playing, playback.loading);
    let progress = if playback.duration > 0.0 {
        (app.player.position() / playback.duration).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let title = track
        .as_ref()
        .map_or("Nothing playing".to_string(), |t| t.title.clone());
    let artists = track.as_ref().map(|t| runs_text(&t.artists));
    v_flex()
        .size_full()
        .bg(c.base)
        .text_color(c.text)
        .type_body()
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .px(space::LG)
                .gap(space::LG)
                .child(widgets::cover(
                    track
                        .as_ref()
                        .and_then(|t| t.thumbnail.clone())
                        .map(Into::into),
                    COVER,
                    false,
                    &c,
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(space::XS)
                        .child(
                            h_flex()
                                .gap(space::SM)
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .truncate()
                                                .type_label()
                                                .when(track.is_none(), |s| {
                                                    s.text_color(c.text_faint)
                                                })
                                                .child(title),
                                        )
                                        .children(artists.map(|a| widgets::muted_line(a, &c))),
                                )
                                .child(expand(&c, cx)),
                        )
                        .child(transport(playing, loading, &c, cx)),
                ),
        )
        .child(
            div()
                .w_full()
                .h(PROGRESS)
                .bg(c.raised)
                .child(div().h_full().w(relative(progress)).bg(c.signal)),
        )
}

/// Back to the full window.
fn expand(c: &Colors, cx: &mut Context<MiniPlayer>) -> impl IntoElement {
    widgets::icon_button(
        "mini-expand",
        widgets::icon(IconName::Maximize2, size::ICON_SM, c.text_muted),
        c,
    )
    .tooltip(widgets::tooltip("Open the full window"))
    .on_click(cx.listener(|this, _, _, cx| {
        this.app.read(cx).desktop.remote.request(Request::Show);
    }))
}

fn transport(
    playing: bool,
    loading: bool,
    c: &Colors,
    cx: &mut Context<MiniPlayer>,
) -> impl IntoElement {
    let send = |command: fn() -> Command| {
        move |this: &mut MiniPlayer,
              _: &ClickEvent,
              _: &mut Window,
              cx: &mut Context<MiniPlayer>| {
            this.app.update(cx, |app, _| app.send(command()));
        }
    };
    let fg = c.primary_foreground;
    let play = if loading {
        Spinner::new().color(fg).into_any_element()
    } else if playing {
        widgets::glyph(Glyph::Pause, size::ICON, fg).into_any_element()
    } else {
        widgets::glyph(Glyph::Play, size::ICON, fg).into_any_element()
    };
    let hover = c.primary_hover;
    h_flex()
        .gap(space::XS)
        .child(
            widgets::icon_button(
                "mini-previous",
                widgets::glyph(Glyph::SkipBack, px(18.), c.text),
                c,
            )
            .tooltip(widgets::tooltip("Previous"))
            .on_click(cx.listener(send(|| Command::Previous))),
        )
        .child(
            h_flex()
                .id("mini-play")
                .size(size::PLAY_BUTTON)
                .justify_center()
                .rounded(radius::FULL)
                .bg(c.primary)
                .when(!playing && !loading, |s| s.pl(px(2.)))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(|s| s.opacity(0.9))
                .child(play)
                .tooltip(widgets::tooltip("Play or pause"))
                .on_click(cx.listener(send(|| Command::TogglePause))),
        )
        .child(
            widgets::icon_button(
                "mini-next",
                widgets::glyph(Glyph::SkipForward, px(18.), c.text),
                c,
            )
            .tooltip(widgets::tooltip("Next"))
            .on_click(cx.listener(send(|| Command::Next))),
        )
}
