//! Audition: holding Alt (alone) over a song row or card, or holding the
//! middle button on it, previews the song's best part over the ducked
//! current one; letting go brings the current song back. Rows and cards
//! report hover and the middle button (`views::extras::audition`); the root
//! reports Alt. Alt pressed with another key is a shortcut (Alt+← Back), so
//! it auditions nothing until Alt is let go.

use encore_core::backend::Command;
use encore_core::model::{Audition, Track};
use gpui_kit::*;

use crate::app::MusicApp;

/// The audition the backend reports, for rows and cards to mark.
pub struct Shown(pub Option<Audition>);

impl Global for Shown {}

/// What is held now.
#[derive(Default)]
pub(crate) struct Hold {
    /// The song row or card under the pointer.
    hovered: Option<Track>,
    alt: bool,
    middle: bool,
    /// Alt went down with another key: no audition until Alt is up.
    blocked: bool,
    /// The video id auditioned.
    auditioning: Option<String>,
}

/// Where a held song starts when YouTube has no most-replayed part for it:
/// a third of the way in, or 30 s into long songs (mixes, extended
/// versions), whose thirds are far from their hooks.
fn best_part(track: &Track) -> Option<f64> {
    let duration = f64::from(track.duration?);
    Some(if duration > 360.0 {
        30.0
    } else {
        duration / 3.0
    })
}

impl MusicApp {
    /// A song row or card gained or lost the pointer.
    pub(crate) fn audition_hover(&mut self, track: &Track, hovered: bool, cx: &mut Context<Self>) {
        let hold = &mut self.extras.hold;
        if hovered {
            hold.hovered = Some(track.clone());
        } else if hold
            .hovered
            .as_ref()
            .is_some_and(|t| t.video_id == track.video_id)
        {
            hold.hovered = None;
        }
        self.audition_update(cx);
    }

    /// The middle button went down on a song.
    pub(crate) fn audition_middle(&mut self, track: &Track, cx: &mut Context<Self>) {
        self.extras.hold.middle = true;
        self.extras.hold.hovered = Some(track.clone());
        self.audition_update(cx);
    }

    /// Lets go of everything (the window lost the focus).
    pub(super) fn audition_release(&mut self, cx: &mut Context<Self>) {
        let hold = &mut self.extras.hold;
        if hold.alt || hold.middle {
            hold.alt = false;
            hold.middle = false;
            self.audition_update(cx);
        }
    }

    fn audition_modifiers(&mut self, m: &Modifiers, cx: &mut Context<Self>) {
        let hold = &mut self.extras.hold;
        hold.alt = m.alt && !m.control && !m.shift && !m.platform;
        if !m.alt {
            hold.blocked = false;
        }
        self.audition_update(cx);
    }

    /// Turns what is held into commands.
    fn audition_update(&mut self, cx: &mut Context<Self>) {
        let hold = &self.extras.hold;
        let held = hold
            .hovered
            .clone()
            .filter(|_| hold.middle || (hold.alt && !hold.blocked));
        match held {
            Some(track) if hold.auditioning.as_deref() != Some(track.video_id.as_str()) => {
                // The best part: the most replayed point when YouTube's heat
                // for the song is known, else a guess from its length.
                let start = self
                    .extras
                    .heat
                    .get(&track.video_id)
                    .and_then(|h| h.as_ref()?.peak)
                    .map(|peak| peak.start)
                    .or_else(|| best_part(&track));
                log::info!(
                    "audition {} ({}) from {}",
                    track.title,
                    track.video_id,
                    start.map_or("a third in".into(), |s| format!("{s:.0}s"))
                );
                self.extras.hold.auditioning = Some(track.video_id.clone());
                self.send(Command::Audition { track, start });
                cx.notify();
            }
            Some(_) => {}
            None => {
                if let Some(id) = self.extras.hold.auditioning.take() {
                    log::info!("audition of {id} let go");
                    self.send(Command::EndAudition);
                    cx.notify();
                }
            }
        }
    }
}

/// The root's listeners: Alt, keys pressed with it, and the middle button
/// coming up anywhere.
pub(super) fn listen_root(root: Div, cx: &mut Context<MusicApp>) -> Div {
    root.on_modifiers_changed(cx.listener(|this, e: &ModifiersChangedEvent, _, cx| {
        this.audition_modifiers(&e.modifiers, cx)
    }))
    .capture_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
        let modifier_key = matches!(
            e.keystroke.key.as_str(),
            "alt" | "shift" | "control" | "platform" | "function"
        );
        if e.keystroke.modifiers.alt && !modifier_key && !this.extras.hold.blocked {
            this.extras.hold.blocked = true;
            this.audition_update(cx);
        }
    }))
    .on_mouse_up(
        MouseButton::Middle,
        cx.listener(|this, _, _, cx| {
            this.extras.hold.middle = false;
            this.audition_update(cx);
        }),
    )
    .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
        // The button came up outside the window.
        if this.extras.hold.middle && e.pressed_button != Some(MouseButton::Middle) {
            this.extras.hold.middle = false;
            this.audition_update(cx);
        }
    }))
}
