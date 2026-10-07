//! Audition on song rows and cards: they report the pointer and the middle
//! button (`listen`), and mark the song while it is auditioned (`ring`): a
//! ring in signal breathes around it while it plays, and holds still,
//! fainter, while it is being prepared. The breath follows the motion
//! speed and holds still under reduced motion (Settings → Motion).

use std::time::Duration;

use encore_core::model::Track;
use gpui_kit::*;

use crate::app::MusicApp;
use crate::extras::Shown;
use crate::theme::{self, motion, radius};

/// The ring's gap outside the element, and its line.
const GAP: Pixels = px(3.);
const LINE: Pixels = px(2.);

/// A song row: listens for audition and rings itself (`corner` is its
/// radius). Rows without a song are left as they are.
pub fn hook(
    el: Stateful<Div>,
    track: Option<&Track>,
    corner: Pixels,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    let Some(track) = track else { return el };
    let ring = ring(Some(track), corner, cx);
    listen(el.relative(), Some(track), cx).children(ring)
}

/// Reports the pointer over a song and the middle button pressed on it.
pub fn listen(
    el: Stateful<Div>,
    track: Option<&Track>,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    let Some(track) = track else { return el };
    let hovered = track.clone();
    let pressed = track.clone();
    el.hover_listener_mode(HoverListenerMode::InputModalityIndependent)
        .on_hover(cx.listener(move |this, on: &bool, _, cx| {
            this.audition_hover(&hovered, *on, cx);
            // Resting on a song resolves it ahead of a click (M28).
            let want = crate::pages::Want::Song(hovered.video_id.clone());
            this.hover_intent(want, *on, cx);
        }))
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(move |this, _, _, cx| this.audition_middle(&pressed, cx)),
        )
}

/// The ring around a song's element (`corner`: its radius; a round cover
/// passes `radius::FULL`) while it is auditioned. Its parent must be
/// `relative`.
pub fn ring(track: Option<&Track>, corner: Pixels, cx: &App) -> Option<AnyElement> {
    let id = &track?.video_id;
    let shown = cx.try_global::<Shown>()?.0.as_ref()?;
    if &shown.video_id != id {
        return None;
    }
    let c = theme::colors(cx);
    let outer = if corner >= radius::FULL {
        corner
    } else {
        corner + GAP
    };
    let ring = div()
        .absolute()
        .top(-GAP)
        .left(-GAP)
        .right(-GAP)
        .bottom(-GAP)
        .rounded(outer)
        .border(LINE)
        .border_color(c.signal);
    let breath = motion::duration(Duration::from_millis(1100));
    Some(match breath {
        Some(breath) if shown.playing => ring
            .with_animation(
                SharedString::from(format!("audition-ring:{id}")),
                Animation::new(breath)
                    .repeat()
                    .with_easing(pulsating_between(0.35, 1.)),
                |el, t| el.opacity(t),
            )
            .into_any_element(),
        None if shown.playing => ring.opacity(0.8).into_any_element(),
        _ => ring.opacity(0.3).into_any_element(),
    })
}
