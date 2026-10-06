//! Stage's lyrics in large type. Timed lyrics follow the song: the current
//! line bright about a third of the way down, the next lines readable, past
//! lines faint; clicking a line seeks to it. Plain lyrics are shown as text
//! to scroll.

use std::time::Duration;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::Lyrics;

use crate::app::MusicApp;
use crate::theme::{self, Colors, Type, radius, space};

/// Where the current line sits in the view, from the top.
const ANCHOR: f32 = 0.36;
/// How much of the way to the target each frame scrolls (an ease out).
const EASE: f32 = 0.16;

pub fn lyrics(
    app: &mut MusicApp,
    id: &str,
    lyrics: &Lyrics,
    window_height: f32,
    window: &mut Window,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let size = (window_height * 0.046).clamp(26., 52.);
    if lyrics.lines.is_empty() {
        return plain(id, lyrics, size, c);
    }
    let position = app.player.position();
    let current = ytfast::lyrics::current_line(&lyrics.lines, position);
    follow(app, id, current, window);
    wake_for_next(app, lyrics, current, position, cx);
    let handle = app.extras.stage.scroll.clone();
    let lines = lyrics.lines.iter().enumerate().map(|(i, line)| {
        let fg = match current {
            Some(now) if i == now => c.text,
            Some(now) if i < now => c.text_faint.opacity(0.5),
            _ => c.text_muted.opacity(0.7),
        };
        let start = line.start;
        let text = if line.text.trim().is_empty() {
            "♪".to_string()
        } else {
            line.text.clone()
        };
        let hover = c.text;
        div()
            .id(("stage-lyric", i))
            .px(space::MD)
            .py(px(size * 0.22))
            .rounded(radius::LG)
            .font_family(theme::FONT_DISPLAY)
            .font_weight(FontWeight::BOLD)
            .text_size(px(size))
            .line_height(px(size * 1.18))
            .text_color(fg)
            .cursor_pointer()
            .hover(move |s| s.text_color(hover))
            .child(text)
            .on_click(cx.listener(move |this, _, _, cx| this.seek_to(start, cx)))
    });
    v_flex()
        .id("stage-lyrics")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .track_scroll(&handle)
        .pb(px(window_height * 0.6))
        .children(lines)
        .children(lyrics.source.clone().map(|source| {
            div()
                .px(space::MD)
                .pt(space::XL)
                .type_caption()
                .text_color(c.text_faint)
                .child(source)
        }))
        .into_any_element()
}

/// Eases the view toward the current line.
fn follow(app: &mut MusicApp, id: &str, current: Option<usize>, window: &mut Window) {
    let stage = &mut app.extras.stage;
    if stage.song.as_deref() != Some(id) {
        stage.song = Some(id.to_string());
        stage.scroll.set_offset(point(px(0.), px(0.)));
    }
    let Some(line) = current.and_then(|i| stage.scroll.bounds_for_item(i)) else {
        if current.is_some() {
            window.request_animation_frame();
        }
        return;
    };
    let view = stage.scroll.bounds();
    let max = stage.scroll.max_offset().y;
    let target = (view.top() + view.size.height * ANCHOR - line.top() - line.size.height / 2.)
        .clamp(-max, px(0.));
    let now = stage.scroll.offset();
    let gap = target - now.y;
    if gap.abs() < px(0.5) {
        return;
    }
    let step = if gap.abs() < px(1.) { gap } else { gap * EASE };
    stage.scroll.set_offset(point(now.x, now.y + step));
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
    let stage = &mut app.extras.stage;
    if !playing {
        stage.wake = None;
        return;
    }
    let next = current.map_or(0, |i| i + 1);
    let Some(line) = lyrics.lines.get(next) else {
        return;
    };
    if stage.wake.as_ref().is_some_and(|(i, _)| *i == next) {
        return;
    }
    let delay = Duration::from_secs_f64((line.start - position).max(0.0) + 0.02);
    let task = cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        let _ = this.update(cx, |_, cx| cx.notify());
    });
    stage.wake = Some((next, task));
}

fn plain(id: &str, lyrics: &Lyrics, size: f32, c: &Colors) -> AnyElement {
    let size = size * 0.62;
    v_flex()
        .id(SharedString::from(format!("stage-plain:{id}")))
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(space::MD)
        .pb(space::XXXL)
        .gap(space::XS)
        .children(lyrics.text.lines().map(|line| {
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(size))
                .line_height(px(size * 1.3))
                .text_color(c.text_muted)
                .when(line.trim().is_empty(), |s| s.h(space::MD))
                .child(line.to_string())
        }))
        .into_any_element()
}
