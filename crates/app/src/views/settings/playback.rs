//! Settings → Playback: loudness levelling, smooth mixes (`mixes`), the
//! sleep timer (`sleep`), notifications, Discord Rich Presence (M34) and loading pages on hover (M28),
//! and their Reset.

use encore_core::model::Mixes;
use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::Colors;

pub fn page(
    app: &MusicApp,
    c: &Colors,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    let mut sections = vec![
        loudness(app, c, cx),
        super::mixes::section(app, c, window, cx),
        super::sleep::section(app, c, cx),
        notifications(app, c, cx),
    ];
    // A build without a Discord application has nothing to offer there.
    if encore_core::discord::configured() {
        sections.push(discord(app, c, cx));
    }
    sections.push(prefetch(app, c, cx));
    sections
}

/// Loudness, mixes or notifications differ from how Music starts out.
pub fn changed(app: &MusicApp) -> bool {
    !app.player.playback.normalize
        || app.player.playback.mixes != Mixes::default()
        || app.notifications()
        || app.discord()
        || !app.pages.prefetch.on
}

pub fn reset(app: &mut MusicApp, cx: &mut Context<MusicApp>) {
    app.set_normalize(true, cx);
    app.set_mixes(Mixes::default(), cx);
    app.set_notifications(false, cx);
    app.set_discord(false, cx);
    app.set_prefetch(true, cx);
}

fn loudness(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
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

fn notifications(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let control = widgets::switch("notifications", app.notifications(), c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_notifications(*on, cx)));
    super::section(
        "Notifications",
        c,
        [super::row(
            "Show a notification when the song changes",
            Some("Only while Encore's window isn't in front".into()),
            control,
            c,
        )],
    )
}

/// What the Discord row says under its name: only claims a state that was
/// checked (Discord answered, or wasn't found).
fn discord_detail(app: &MusicApp) -> &'static str {
    use encore_core::discord::Status;
    match (app.discord(), app.desktop.flags.discord.status()) {
        (false, _) => "Only talks to the Discord app on this computer",
        (true, Status::Connected) => "Connected to Discord on this computer",
        (true, Status::Waiting) => "Discord isn't running. Encore looks again every 30 seconds",
        (true, _) => "Shows while a song plays. Only talks to the Discord app on this computer",
    }
}

fn discord(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let control = widgets::switch("discord", app.discord(), c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_discord(*on, cx)));
    super::section(
        "Discord",
        c,
        [super::row(
            "Show what you're playing in Discord",
            Some(discord_detail(app).into()),
            control,
            c,
        )],
    )
}

fn prefetch(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let control = widgets::switch("prefetch", app.pages.prefetch.on, c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_prefetch(*on, cx)));
    super::section(
        "Loading ahead",
        c,
        [super::row(
            "Load pages when you point at them",
            Some("Albums and playlists open at once, and Play starts sooner".into()),
            control,
            c,
        )],
    )
}
