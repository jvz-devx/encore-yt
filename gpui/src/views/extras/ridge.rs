//! The most-replayed ridge: YouTube's replay heat as a low, smooth hill
//! rising from the seek bar, lit in signal behind the playhead, with a mark
//! on the most replayed part (a click or P jumps there).

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::{clock, widgets};
use crate::app::MusicApp;
use crate::theme::{Colors, radius};

/// The ridge's height where the heat is greatest.
const HEIGHT: f32 = 11.;
/// From the slider's bottom to the top of its track (the kit slider is 24
/// tall with a 6 px track in the middle).
const TRACK_TOP: Pixels = px(15.);
/// The peak mark's hit area.
const MARK: Pixels = px(16.);

/// The ridge over the seek slider, or `None` while the song has no heat.
/// It sits in the slider's box (`relative`), under the slider.
pub fn ridge(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    let duration = app.player.playback.duration;
    let heat = app.current_heat().filter(|_| duration > 0.0)?.clone();
    let played = (app.player.position() / duration).clamp(0.0, 1.0) as f32;
    let colors = *c;
    let peak = heat.peak;
    let crest = peak.map(|p| heat.value_at(p.at)).unwrap_or(0.);
    let curve = heat.curve.clone();
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(TRACK_TOP)
            .h(px(HEIGHT + 2.))
            .child(
                canvas(
                    |_, _, _| {},
                    // The effects layer draws it while it paints the bar.
                    move |bounds, _, window, cx| {
                        if !crate::visuals::paints_bar(cx) {
                            paint(bounds, &curve, duration, played, &colors, window)
                        }
                    },
                )
                .size_full(),
            )
            .when_some(peak, |el, peak| {
                let at = (peak.at / duration).clamp(0.0, 1.0) as f32;
                let tip = format!("Most replayed, {} (P)", clock(peak.start));
                el.child(
                    div()
                        .id("most-replayed")
                        .absolute()
                        .left(relative(at))
                        .ml(-MARK / 2.)
                        .bottom(px(HEIGHT * crest) - MARK / 2. + px(1.))
                        .size(MARK)
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .child(div().size(px(5.)).rounded(radius::FULL).bg(c.text))
                        .tooltip(widgets::tooltip(tip))
                        .on_click(cx.listener(|this, _, _, cx| this.jump_to_peak(cx))),
                )
            })
            .into_any_element(),
    )
}

/// The hill, split at the playhead: signal behind it, muted ahead.
fn paint(
    bounds: Bounds<Pixels>,
    curve: &[(f64, f32)],
    duration: f64,
    played: f32,
    c: &Colors,
    window: &mut Window,
) {
    let w = f32::from(bounds.size.width);
    let base = f32::from(bounds.bottom());
    let left = f32::from(bounds.left());
    let x = |t: f64| left + (t / duration).clamp(0.0, 1.0) as f32 * w;
    let tops: Vec<Point<Pixels>> = curve
        .iter()
        .map(|&(t, v)| point(px(x(t)), px(base - HEIGHT * v)))
        .collect();
    let split = left + w * played;
    let halves = [
        (left, split, c.signal.opacity(0.42), c.signal),
        (
            split,
            left + w,
            c.text_muted.opacity(0.16),
            c.text_muted.opacity(0.5),
        ),
    ];
    for (from, to, fill_color, edge) in halves {
        if to - from < 0.5 {
            continue;
        }
        let mask = ContentMask {
            bounds: Bounds::from_corners(
                point(px(from), bounds.top() - px(4.)),
                point(px(to), bounds.bottom()),
            ),
        };
        window.with_content_mask(Some(mask), |window| {
            let mut area = PathBuilder::fill();
            area.move_to(point(px(left), px(base)));
            for p in &tops {
                area.line_to(*p);
            }
            area.line_to(point(px(left + w), px(base)));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, fill_color);
            }
            let mut line = PathBuilder::stroke(px(1.));
            if let Some(first) = tops.first() {
                line.move_to(*first);
            }
            for p in tops.iter().skip(1) {
                line.line_to(*p);
            }
            if let Ok(path) = line.build() {
                window.paint_path(path, edge);
            }
        });
    }
}
