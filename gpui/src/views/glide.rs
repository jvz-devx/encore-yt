//! Timed lyrics that glide (Settings → Motion → Lyrics), shared by Now
//! Playing's Lyrics tab and Stage: the current line grows and brightens,
//! past lines dim and shrink a little, upcoming ones are dimmed, lines
//! further away fade more, and the current line fills from left to right
//! as it is sung. The view scrolls smoothly so the current line stays at
//! its anchor.
//!
//! Brightness is the text colour's opacity, so both themes work from the
//! one `text` token. A line moves from how it looked under the previous
//! line to how it looks now, keyed by the current line, so each change of
//! line plays once.

use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::LyricLine;

use crate::theme::motion::{self, Align, Kind, Lyrics, MotionExt as _};

/// How many characters the sweep's soft edge spans.
const FEATHER: usize = 4;
/// The sweep redraws at most this often, and at least this often while
/// the line is sung (a long line still moves).
const SWEEP_FASTEST: f64 = 0.04;
const SWEEP_SLOWEST: f64 = 0.25;
/// How long the last line counts as sung for, when the song's length is
/// unknown.
const LAST_LINE: f64 = 4.0;
/// How much of the way to its target the view scrolls each frame at
/// normal speed (an ease out).
const EASE: f32 = 0.18;
/// How faint the farthest lines get against the near ones.
const FAR_FLOOR: f32 = 0.3;
const FAR_STEP: f32 = 0.14;

/// How a line looks: its size against the base and its text's opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Look {
    scale: f32,
    alpha: f32,
}

impl Look {
    fn mix(self, to: Look, t: f32) -> Look {
        Look {
            scale: self.scale + (to.scale - self.scale) * t,
            alpha: self.alpha + (to.alpha - self.alpha) * t,
        }
    }
}

/// The settings in effect, and whether lines move.
#[derive(Clone, Copy)]
pub struct Glide {
    pub lyrics: Lyrics,
    moves: bool,
}

impl Glide {
    pub fn now() -> Self {
        Self {
            lyrics: motion::config().lyrics,
            moves: motion::enabled(Kind::Lyrics),
        }
    }

    /// The base text size for `medium`, at the chosen text size.
    pub fn size(&self, medium: f32) -> f32 {
        medium * self.lyrics.size.factor()
    }

    /// How line `i` looks while line `current` is sung.
    fn look(&self, i: usize, current: Option<usize>) -> Look {
        let l = &self.lyrics;
        let far = |distance: usize| {
            if l.fade_far {
                (1. - FAR_STEP * distance.saturating_sub(1) as f32).max(FAR_FLOOR)
            } else {
                1.
            }
        };
        match current {
            Some(now) if i == now => Look {
                scale: l.scale,
                alpha: 1.,
            },
            Some(now) if i < now => Look {
                // Past lines shrink by a quarter of the current line's
                // growth.
                scale: 1. - (l.scale - 1.) * 0.25,
                alpha: (1. - l.dim) * far(now - i),
            },
            _ => Look {
                scale: 1.,
                alpha: (1. - l.dim * 0.6) * far(current.map_or(i + 1, |now| i - now)),
            },
        }
    }
}

/// What a line needs to be drawn.
pub struct Line<'a> {
    /// The line's own element: padding, corners, font, click; no size or
    /// colour, which come from here.
    pub el: Stateful<Div>,
    /// Unique to the view and line (`"lyric"`, `"stage-lyric"`).
    pub prefix: &'static str,
    pub index: usize,
    pub lines: &'a [LyricLine],
    pub current: Option<usize>,
    pub position: f64,
    /// The song's length, for how long the last line is sung.
    pub duration: f64,
    /// The base text size and the line height against it.
    pub size: f32,
    pub leading: f32,
    pub text: Hsla,
}

/// Line `index`, looking as it should now and moving there from how it
/// looked under the line before.
pub fn line(glide: &Glide, line: Line<'_>) -> AnyElement {
    let i = line.index;
    let to = glide.look(i, line.current);
    let from = glide.look(i, line.current.and_then(|now| now.checked_sub(1)));
    let words = line.lines.get(i).map_or("", |l| l.text.as_str());
    let words = if words.trim().is_empty() {
        "♪".to_string()
    } else {
        words.to_string()
    };
    let sweep = (glide.lyrics.sweep && line.current == Some(i))
        .then(|| sung(line.lines, i, line.position, line.duration));
    // The unsung part of the current line is as dim as an upcoming line:
    // that is the line's own colour, and the sung part is painted over it
    // (GPUI blends a highlight's colour over the text's).
    let unsung = sweep.map(|_| glide.look(i + 1, Some(i)).alpha);
    let content = match sweep {
        Some(p) => StyledText::new(words.clone())
            .with_highlights(sweep_runs(&words, p, line.text))
            .into_any_element(),
        None => words.into_any_element(),
    };
    let (size, leading, text) = (line.size, line.leading, line.text);
    let el = line
        .el
        .when(glide.lyrics.align == Align::Centre, |el| el.text_center())
        .child(content);
    let style = move |el: Stateful<Div>, look: Look| {
        let px_size = size * look.scale;
        el.text_size(px(px_size))
            .line_height(px(px_size * leading))
            .text_color(text.opacity(unsung.unwrap_or(look.alpha)))
    };
    if !glide.moves || from == to {
        return style(el, to).into_any_element();
    }
    let now = line.current.map_or(0, |n| n + 1);
    el.with_motion(
        SharedString::from(format!("{}-glide:{i}:{now}", line.prefix)),
        Kind::Lyrics,
        motion::SLOW,
        move |el, t| style(el, from.mix(to, t)),
    )
}

/// How far into line `i` the song is, from 0 to 1.
fn sung(lines: &[LyricLine], i: usize, position: f64, duration: f64) -> f32 {
    let Some(line) = lines.get(i) else {
        return 0.;
    };
    let end = lines.get(i + 1).map_or_else(
        || {
            if duration > line.start {
                (line.start + LAST_LINE).min(duration)
            } else {
                line.start + LAST_LINE
            }
        },
        |next| next.start,
    );
    let length = (end - line.start).max(0.1);
    ((position - line.start) / length).clamp(0., 1.) as f32
}

/// The colours of a line sung `p` of the way: bright up to the edge, a
/// soft edge a few characters wide, then nothing over the line's own
/// (unsung) colour.
fn sweep_runs(words: &str, p: f32, text: Hsla) -> Vec<(std::ops::Range<usize>, HighlightStyle)> {
    let chars: Vec<(usize, char)> = words.char_indices().collect();
    let n = chars.len();
    let edge = p * (n + FEATHER) as f32;
    let colour = |fill: f32| HighlightStyle {
        color: Some(text.opacity(fill)),
        ..Default::default()
    };
    let end_of = |k: usize| chars.get(k + 1).map_or(words.len(), |(b, _)| *b);
    let mut runs = Vec::new();
    // Characters wholly sung, then the edge one by one; the rest keeps
    // the line's colour.
    let solid = (edge - FEATHER as f32).floor().clamp(0., n as f32) as usize;
    if solid > 0 {
        runs.push((0..end_of(solid - 1), colour(1.)));
    }
    let soft_end = (edge.ceil() as usize).min(n);
    for (k, &(at, _)) in chars.iter().enumerate().take(soft_end).skip(solid) {
        let fill = ((edge - k as f32) / FEATHER as f32).clamp(0., 1.);
        runs.push((at..end_of(k), colour(fill)));
    }
    runs
}

/// When the view should next redraw by itself, and a key for that moment
/// (so a timer already waiting for it isn't started again): the next step
/// of the sweep, or the next line.
pub fn next_wake(
    glide: &Glide,
    lines: &[LyricLine],
    current: Option<usize>,
    position: f64,
    duration: f64,
) -> Option<(usize, Duration)> {
    let next = current.map_or(0, |i| i + 1);
    let to_next = lines.get(next).map(|l| (l.start - position).max(0.));
    let sweep = current.filter(|_| glide.lyrics.sweep).and_then(|i| {
        let p = sung(lines, i, position, duration);
        if p >= 1. {
            return None;
        }
        let steps = (lines[i].text.chars().count() + FEATHER) as f64;
        let end = lines
            .get(i + 1)
            .map_or(LAST_LINE, |l| l.start - lines[i].start);
        let step = (end / steps).clamp(SWEEP_FASTEST, SWEEP_SLOWEST);
        let k = (f64::from(p) * end / step).floor() as usize + 1;
        Some((i * 4096 + k, step - (f64::from(p) * end) % step))
    });
    let line = to_next.map(|delay| (next * 4096, delay));
    let (key, delay) = match (sweep, line) {
        (Some(s), Some(l)) => {
            if s.1 < l.1 {
                s
            } else {
                l
            }
        }
        (s, l) => s.or(l)?,
    };
    Some((key, Duration::from_secs_f64(delay + 0.02)))
}

/// Scrolls `handle` toward line `current` sitting at the anchor: eased
/// when lines glide, at once when they don't.
pub fn follow(glide: &Glide, handle: &ScrollHandle, current: Option<usize>, window: &mut Window) {
    let Some(line) = current.and_then(|i| handle.bounds_for_item(i)) else {
        // Not laid out yet: look again next frame.
        if current.is_some() {
            window.request_animation_frame();
        }
        return;
    };
    let view = handle.bounds();
    let max = handle.max_offset().y;
    let anchor = glide.lyrics.anchor.share();
    let target = (view.top() + view.size.height * anchor - line.top() - line.size.height / 2.)
        .clamp(-max, px(0.));
    let now = handle.offset();
    let gap = target - now.y;
    if gap.abs() < px(0.5) {
        return;
    }
    let ease = (EASE * motion::config().speed).min(1.);
    let step = if !glide.moves || gap.abs() < px(1.) {
        gap
    } else {
        gap * ease
    };
    handle.set_offset(point(now.x, now.y + step));
    window.request_animation_frame();
}
