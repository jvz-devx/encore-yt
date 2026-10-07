//! Settings → Playback (loudness levelling) and Notifications.

use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::Colors;

pub fn section(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let playback = &app.player.playback;
    let on = playback.normalize;
    let detail = match (on, playback.gain) {
        (true, Some(gain)) => format!("This song plays at {gain:+.1} dB"),
        _ => "Quiet and loud songs play at a similar volume".into(),
    };
    let control = widgets::switch("normalize", on, c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_normalize(*on, cx)));
    super::section(
        "Playback",
        c,
        [super::row(
            "Even out loudness between songs",
            Some(detail.into()),
            control,
            c,
        )],
    )
}

pub fn notifications(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let control = widgets::switch("notifications", app.notifications(), c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_notifications(*on, cx)));
    super::section(
        "Notifications",
        c,
        [super::row(
            "Show a notification when the song changes",
            Some("Only while Music's window isn't in front".into()),
            control,
            c,
        )],
    )
}
