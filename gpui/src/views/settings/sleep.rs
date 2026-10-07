//! Settings → Sleep timer: what is set, the timers offered (as in the
//! player bar's menu), and Turn off.

use gpui_kit::*;

use super::super::extras::{SLEEP_CHOICES, time_left};
use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::theme::Colors;

pub fn section(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let timer = app.player.playback.sleep;
    let chosen = timer.map(|t| t.choice);
    let (label, detail) = match &timer {
        Some(t) if t.deadline.is_some() => (format!("Pauses in {}", time_left(t)), None),
        Some(_) => ("Pauses when this song ends".to_string(), None),
        None => (
            "No timer set".to_string(),
            Some("Fades out, then pauses playback".into()),
        ),
    };
    let off = super::focusable(
        widgets::pill_button("settings-sleep-off", "Turn off", None, Pill::Tonal, c),
        c,
    )
    .on_click(cx.listener(|this, _, _, cx| this.sleep(None, cx)));
    let timers = SLEEP_CHOICES
        .into_iter()
        .enumerate()
        .map(|(i, (name, choice))| {
            super::choice(("sleep-timer", i), name, chosen == Some(choice), c)
                .on_click(cx.listener(move |this, _, _, cx| this.sleep(Some(choice), cx)))
        });
    super::section(
        "Sleep timer",
        c,
        [
            super::row(
                label,
                detail,
                div().children(timer.is_some().then_some(off)),
                c,
            ),
            super::choices(timers),
        ],
    )
}
