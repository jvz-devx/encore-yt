//! Stage's lyrics in large type. Timed lyrics follow the song and glide as
//! Settings → Motion → Lyrics says (`views::glide`): the current line
//! larger and lit at its anchor, the others dimmed; clicking a line seeks
//! to it. Plain lyrics are shown as text to scroll.

use encore_core::model::Lyrics;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::glide::{self, Glide};
use crate::app::MusicApp;
use crate::theme::motion::Align;
use crate::theme::{self, Colors, Type, radius, space};

/// A line's height against its text size.
const LEADING: f32 = 1.18;

pub fn lyrics(
    app: &mut MusicApp,
    id: &str,
    lyrics: &Lyrics,
    window_height: f32,
    window: &mut Window,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let glide = Glide::now();
    let size = glide.size((window_height * 0.046).clamp(26., 52.));
    if lyrics.lines.is_empty() {
        return plain(id, lyrics, size, c);
    }
    let position = app.player.position();
    let duration = app.player.playback.duration;
    let current = encore_core::lyrics::current_line(&lyrics.lines, position);
    follow(app, id, current, &glide, window);
    wake(app, &glide, lyrics, current, position, cx);
    let handle = app.extras.stage.scroll.clone();
    let hover = c.text;
    let lines = (0..lyrics.lines.len()).map(|i| {
        let start = lyrics.lines[i].start;
        let el = div()
            .id(("stage-lyric", i))
            .px(space::MD)
            .py(px(size * 0.22))
            .rounded(radius::LG)
            .font_family(theme::FONT_DISPLAY)
            .font_weight(FontWeight::BOLD)
            .cursor_pointer()
            .hover(move |s| s.text_color(hover))
            .on_click(cx.listener(move |this, _, _, cx| this.seek_to(start, cx)));
        glide::line(
            &glide,
            glide::Line {
                el,
                prefix: "stage-lyric",
                index: i,
                lines: &lyrics.lines,
                current,
                position,
                duration,
                size,
                leading: LEADING,
                text: c.text,
            },
        )
    });
    let centred = glide.lyrics.align == Align::Centre;
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
                .when(centred, |el| el.text_center())
                .type_caption()
                .text_color(c.text_faint)
                .child(source)
        }))
        .into_any_element()
}

/// Keeps the current line at its anchor; a new song starts at the top.
fn follow(
    app: &mut MusicApp,
    id: &str,
    current: Option<usize>,
    glide: &Glide,
    window: &mut Window,
) {
    let stage = &mut app.extras.stage;
    if stage.song.as_deref() != Some(id) {
        stage.song = Some(id.to_string());
        stage.scroll.set_offset(point(px(0.), px(0.)));
    }
    glide::follow(glide, &stage.scroll, current, window);
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
    let stage = &mut app.extras.stage;
    if !playing {
        stage.wake = None;
        return;
    }
    let Some((key, delay)) = glide::next_wake(glide, &lyrics.lines, current, position, duration)
    else {
        return;
    };
    if stage.wake.as_ref().is_some_and(|(k, _)| *k == key) {
        return;
    }
    let task = cx.spawn(async move |this, cx| {
        cx.background_executor().timer(delay).await;
        let _ = this.update(cx, |_, cx| cx.notify());
    });
    stage.wake = Some((key, task));
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
