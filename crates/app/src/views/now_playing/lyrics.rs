//! The Lyrics tab. Timed lyrics follow the song and glide as Settings →
//! Motion → Lyrics says (`views::glide`): the current line larger and lit
//! at its anchor, the others dimmed. A hand scroll holds the view for a
//! few seconds; clicking a line seeks to it. Plain lyrics scroll as text.

use std::time::Instant;

use encore_core::model::Lyrics;
use gpui_kit::assets::IconName;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::glide::{self, Glide};
use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::playback::LYRICS_HOLD;
use crate::theme::motion::Align;
use crate::theme::{self, Colors, Type, radius, size, space};

/// The medium text size of a line, and its line height against its size
/// (the title style, 22/28).
const TITLE: f32 = 22.;
const LEADING: f32 = 28. / 22.;

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
    let glide = Glide::now();
    let position = app.player.position();
    let duration = app.player.playback.duration;
    let current = encore_core::lyrics::current_line(&lyrics.lines, position);
    follow(app, id, current, &glide, window);
    wake(app, &glide, lyrics, current, position, cx);
    let handle = app.player.lyrics_scroll.handle.clone();
    let hover = c.text;
    let lines = (0..lyrics.lines.len()).map(|i| {
        let start = lyrics.lines[i].start;
        let el = div()
            .id(("lyric", i))
            .px(space::SM)
            .py(space::SM)
            .rounded(radius::MD)
            .font_family(theme::FONT_DISPLAY)
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .hover(move |s| s.text_color(hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.player.lyrics_scroll.held_until = None;
                this.seek_to(start, cx);
            }));
        glide::line(
            &glide,
            glide::Line {
                el,
                prefix: "lyric",
                index: i,
                lines: &lyrics.lines,
                current,
                position,
                duration,
                size: glide.size(TITLE),
                leading: LEADING,
                text: c.text,
            },
        )
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
        .children(credit(lyrics, glide.lyrics.align, c))
        .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, _| {
            this.player.lyrics_scroll.held_until = Some(Instant::now() + LYRICS_HOLD);
        }))
}

/// Keeps the current line at its anchor, unless a hand scroll holds the
/// view; a new song starts at the top.
fn follow(
    app: &mut MusicApp,
    id: &str,
    current: Option<usize>,
    glide: &Glide,
    window: &mut Window,
) {
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
    glide::follow(glide, &scroll.handle, current, window);
}

/// Redraws when the next line starts or the sweep moves on, so lines
/// light on time.
fn wake(
    app: &mut MusicApp,
    glide: &Glide,
    lyrics: &Lyrics,
    current: Option<usize>,
    position: f64,
    cx: &mut Context<MusicApp>,
) {
    let playing = app.player.playback.playing && !app.player.playback.loading;
    let duration = app.player.playback.duration;
    let scroll = &mut app.player.lyrics_scroll;
    if !playing {
        scroll.wake = None;
        return;
    }
    let Some((key, delay)) = glide::next_wake(glide, &lyrics.lines, current, position, duration)
    else {
        return;
    };
    if scroll.wake.as_ref().is_some_and(|(k, _)| *k == key) {
        return;
    }
    let task = cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        let _ = this.update(cx, |_, cx| cx.notify());
    });
    scroll.wake = Some((key, task));
}

fn credit(lyrics: &Lyrics, align: Align, c: &Colors) -> Option<impl IntoElement> {
    let source = lyrics.source.clone()?;
    Some(
        div()
            .when(align == Align::Centre, |el| el.text_center())
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
        .children(credit(lyrics, Align::Left, c))
}
