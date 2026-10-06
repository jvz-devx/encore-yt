//! The Lyrics tab. Timed lyrics follow the song: the current line lit
//! about a third of the way down, upcoming lines readable, past lines
//! dimmer. A hand scroll holds the view for a few seconds; clicking a line
//! seeks to it. Plain lyrics scroll as text.

use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::Lyrics;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::playback::LYRICS_HOLD;
use crate::theme::{Colors, Type, radius, size, space};

/// Where the current line sits in the view, from the top.
const ANCHOR: f32 = 0.33;
/// How much of the way to the target each frame scrolls (an ease out).
const EASE: f32 = 0.2;

pub fn lyrics(
    app: &mut MusicApp,
    window: &mut Window,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let Some(id) = app.player.current().map(|t| t.video_id.clone()) else {
        return centred(widgets::empty_state(
            IconName::MicVocal,
            "Nothing playing",
            "Play a song to see its lyrics.",
            c,
        ));
    };
    match app.player.current_lyrics().cloned() {
        None => loading(c).into_any_element(),
        Some(Err(error)) => centred(
            widgets::empty_state(IconName::CircleAlert, "Couldn't load the lyrics", error, c)
                .child(
                    widgets::pill_button(
                        "retry-lyrics",
                        "Try again",
                        Some(widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text)),
                        Pill::Secondary,
                        c,
                    )
                    .mt(space::SM)
                    .on_click(cx.listener(|this, _, _, cx| this.retry_lyrics(cx))),
                ),
        ),
        Some(Ok(None)) => centred(widgets::empty_state(
            IconName::MicVocal,
            "Lyrics aren't available for this song",
            "Neither YouTube Music nor LRCLIB has them.",
            c,
        )),
        Some(Ok(Some(lyrics))) if !lyrics.lines.is_empty() => {
            timed(app, &id, &lyrics, window, c, cx).into_any_element()
        }
        Some(Ok(Some(lyrics))) => plain(&id, &lyrics, c).into_any_element(),
    }
}

fn centred(content: Div) -> AnyElement {
    v_flex()
        .flex_1()
        .justify_center()
        .pb(space::XXXL)
        .child(content)
        .into_any_element()
}

/// Loading: lines of lyrics, pulsing.
fn loading(c: &Colors) -> impl IntoElement {
    let widths = [280., 220., 300., 180., 260., 240.];
    v_flex()
        .px(space::SM)
        .pt(space::XL)
        .gap(space::LG)
        .children(
            widths
                .into_iter()
                .map(|w| widgets::skeleton(px(w), px(22.), radius::XS, c)),
        )
}

fn timed(
    app: &mut MusicApp,
    id: &str,
    lyrics: &Lyrics,
    window: &mut Window,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let position = app.player.position();
    let current = ytfast::lyrics::current_line(&lyrics.lines, position);
    follow(app, id, current, window);
    wake_for_next(app, lyrics, current, position, cx);
    let handle = app.player.lyrics_scroll.handle.clone();
    let lines = lyrics.lines.iter().enumerate().map(|(i, line)| {
        let fg = match current {
            Some(now) if i == now => c.text,
            Some(now) if i < now => c.text_faint.opacity(0.55),
            _ => c.text_muted.opacity(0.75),
        };
        let hover = c.text;
        let start = line.start;
        let text = if line.text.trim().is_empty() {
            "♪".to_string()
        } else {
            line.text.clone()
        };
        div()
            .id(("lyric", i))
            .px(space::SM)
            .py(space::SM)
            .rounded(radius::MD)
            .type_title()
            .text_color(fg)
            .cursor_pointer()
            .hover(move |s| s.text_color(hover))
            .child(text)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.player.lyrics_scroll.held_until = None;
                this.seek_to(start, cx);
            }))
    });
    v_flex()
        .id("lyrics")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&handle)
        .pt(space::XL)
        .pb(px(240.))
        .children(lines)
        .children(credit(lyrics, c))
        .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, _| {
            this.player.lyrics_scroll.held_until = Some(Instant::now() + LYRICS_HOLD);
        }))
}

/// Eases the view toward the current line, unless a hand scroll holds it.
fn follow(app: &mut MusicApp, id: &str, current: Option<usize>, window: &mut Window) {
    let scroll = &mut app.player.lyrics_scroll;
    if scroll.song.as_deref() != Some(id) {
        scroll.song = Some(id.to_string());
        scroll.held_until = None;
        scroll.handle.set_offset(point(px(0.), px(0.)));
    }
    if scroll.held_until.is_some_and(|t| Instant::now() < t) {
        return;
    }
    scroll.held_until = None;
    let Some(line) = current.and_then(|i| scroll.handle.bounds_for_item(i)) else {
        // Not laid out yet: look again next frame.
        if current.is_some() {
            window.request_animation_frame();
        }
        return;
    };
    let view = scroll.handle.bounds();
    let max = scroll.handle.max_offset().y;
    let target = (view.top() + view.size.height * ANCHOR - line.top() - line.size.height / 2.)
        .clamp(-max, px(0.));
    let now = scroll.handle.offset();
    let gap = target - now.y;
    if gap.abs() < px(0.5) {
        return;
    }
    let step = if gap.abs() < px(1.) { gap } else { gap * EASE };
    scroll.handle.set_offset(point(now.x, now.y + step));
    window.request_animation_frame();
}

/// Redraws when the next line starts, so it lights on time.
fn wake_for_next(
    app: &mut MusicApp,
    lyrics: &Lyrics,
    current: Option<usize>,
    position: f64,
    cx: &mut Context<MusicApp>,
) {
    let playing = app.player.playback.playing && !app.player.playback.loading;
    let scroll = &mut app.player.lyrics_scroll;
    if !playing {
        scroll.wake = None;
        return;
    }
    let next = current.map_or(0, |i| i + 1);
    let Some(line) = lyrics.lines.get(next) else {
        return;
    };
    if scroll.wake.as_ref().is_some_and(|(i, _)| *i == next) {
        return;
    }
    let delay = Duration::from_secs_f64((line.start - position).max(0.0) + 0.02);
    let task = cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        let _ = this.update(cx, |_, cx| cx.notify());
    });
    scroll.wake = Some((next, task));
}

fn credit(lyrics: &Lyrics, c: &Colors) -> Option<impl IntoElement> {
    let source = lyrics.source.clone()?;
    Some(
        div()
            .px(space::SM)
            .pt(space::XL)
            .type_caption()
            .text_color(c.text_faint)
            .child(source),
    )
}

fn plain(id: &str, lyrics: &Lyrics, c: &Colors) -> impl IntoElement {
    v_flex()
        .id(SharedString::from(format!("plain-lyrics:{id}")))
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(space::SM)
        .pt(space::SM)
        .pb(space::XXXL)
        .gap(space::XS)
        .children(lyrics.text.lines().map(|line| {
            div()
                .type_heading()
                .text_color(c.text)
                .when(line.trim().is_empty(), |s| s.h(space::MD))
                .child(line.to_string())
        }))
        .children(credit(lyrics, c))
}
