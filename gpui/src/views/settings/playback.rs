//! Settings → Loudness levelling, Audio player and Notifications.

use gpui_kit::*;
use ytfast::player::Kind;

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
        "Loudness levelling",
        c,
        [super::row(
            "Even out loudness between songs",
            Some(detail.into()),
            control,
            c,
        )],
    )
}

/// Which engine plays: the built-in one (mpv for what it can't play) or mpv.
pub fn player(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let playback = &app.player.playback;
    let chosen = playback.player;
    let detail = match (chosen, playback.engine) {
        (Kind::Rust, Some(Kind::Mpv)) => "This song plays on mpv",
        (Kind::Rust, _) => "Plays songs itself, and uses mpv for any it can't play",
        (Kind::Mpv, _) => "Plays songs with mpv",
    };
    let chip = |kind: Kind, label: &'static str| {
        super::choice(
            SharedString::from(format!("player-{}", kind.label())),
            label,
            chosen == kind,
            c,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_player(kind, cx)))
    };
    super::section(
        "Audio player",
        c,
        [
            super::row(
                "Player",
                Some(format!("{detail}. Changes from the next song").into()),
                div().into_any_element(),
                c,
            ),
            super::choices([chip(Kind::Rust, "Built-in"), chip(Kind::Mpv, "mpv")]),
        ],
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
