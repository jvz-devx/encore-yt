//! The page transition (Settings → Motion → Page transitions): a page
//! arriving after a move between views fades, slides or scales in. Going
//! forward it comes from the right, going back from the left, so the
//! motion follows the history.
//!
//! The view drives it frame by frame from `Pages::transition` rather than
//! with `with_animation`, so the page's layout never changes while it
//! moves: offsets are relative positions and the scale is a clip that
//! opens around the page at its full size (GPUI can't transform an
//! element), and the page's list keeps its state and scroll position.

use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::motion::{self, Kind, PageStyle};

/// How far a sliding page travels.
const SLIDE: Pixels = px(72.);
/// How much smaller than the panel a scaling page starts: its leading
/// edge, trailing edge and top and bottom, as shares of the panel.
const SCALE_LEAD: f32 = 0.07;
const SCALE_TRAIL: f32 = 0.015;
const SCALE_V: f32 = 0.035;

/// `page` as it arrives: the panel's page area, moving while the
/// transition runs.
pub fn arrive(app: &MusicApp, page: AnyElement, window: &mut Window) -> AnyElement {
    let frame = div()
        .relative()
        .flex_1()
        .w_full()
        .min_h_0()
        .flex()
        .flex_col();
    let style = motion::config().pages;
    let transition = app.pages.transition;
    let base = match style {
        PageStyle::Fade | PageStyle::None => motion::BASE,
        PageStyle::Slide | PageStyle::Scale => motion::SLOW,
    };
    let Some(t) = motion::progress(Kind::Pages, base, transition.started) else {
        return frame.child(page).into_any_element();
    };
    window.request_animation_frame();
    // Forward arrives from the right, back from the left.
    let side = if transition.back { -1. } else { 1. };
    match style {
        PageStyle::None => frame.child(page).into_any_element(),
        PageStyle::Fade => frame.opacity(t).child(page).into_any_element(),
        PageStyle::Slide => frame
            .left(SLIDE * side * (1. - t))
            .opacity(t)
            .child(page)
            .into_any_element(),
        PageStyle::Scale => frame.child(opening(page, t, side)).into_any_element(),
    }
}

/// The page behind a clip that opens from slightly smaller than the panel
/// to all of it, its centre drifting in from `side`. The page keeps its
/// full size inside, so nothing reflows.
fn opening(page: AnyElement, t: f32, side: f32) -> impl IntoElement {
    let rest = 1. - t;
    let (lead, trail) = (SCALE_LEAD * rest, SCALE_TRAIL * rest);
    // Forward: the larger inset on the left, so the page grows out of the
    // right; back the other way.
    let (left, right) = if side > 0. {
        (lead, trail)
    } else {
        (trail, lead)
    };
    let v = SCALE_V * rest;
    let (w, h) = (1. - left - right, 1. - v * 2.);
    // Inside the clip the page is panel-sized; it is shifted so its centre
    // follows the clip's, so it moves with the opening.
    let shift = (left - right) / 2.;
    let inner = div()
        .absolute()
        .left(relative((-left + shift) / w))
        .top(relative(-v / h + v / h * 0.5))
        .w(relative(1. / w))
        .h(relative(1. / h))
        .flex()
        .flex_col()
        .child(page);
    div()
        .absolute()
        .left(relative(left))
        .right(relative(right))
        .top(relative(v))
        .bottom(relative(v))
        .overflow_hidden()
        .rounded(crate::theme::radius::LG * rest)
        .opacity(t)
        .child(inner)
}
