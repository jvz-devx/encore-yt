//! Settings → Updates: this version, what the last check found (with the
//! release notes and Update and restart, or how to update a copy the app
//! can't replace), Check now, and the daily check and pre-release choices
//! (M16).

use gpui_kit::component::v_flex;
use gpui_kit::*;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::theme::{Colors, Type, space};
use crate::update::{self, State};

/// How many lines of the release notes to show.
const NOTES: usize = 8;

pub fn section(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let updates = &app.updates;
    let mut rows = vec![super::row(
        format!("Music {}", update::VERSION),
        Some(status(app).into()),
        action(app, c, cx),
        c,
    )];
    if let Some(release) = updates.available() {
        let notes = update::notes_excerpt(&release.notes, NOTES);
        if !notes.is_empty() {
            rows.push(
                v_flex()
                    .pb(space::SM)
                    .type_small()
                    .text_color(c.text_muted)
                    .child(notes)
                    .into_any_element(),
            );
        }
    }
    let checks = widgets::switch("update-checks", updates.prefs.check, c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_update_checks(*on, cx)));
    rows.push(super::row(
        "Check for updates",
        Some("Once a day, on GitHub Releases".into()),
        checks,
        c,
    ));
    let pre = widgets::switch("update-prereleases", updates.prefs.prereleases(), c)
        .on_click(cx.listener(|this, on: &bool, _, cx| this.set_prereleases(*on, cx)));
    rows.push(super::row(
        "Include pre-releases",
        Some("Test versions before they're final. On while you use one.".into()),
        pre,
        c,
    ));
    super::section("Updates", c, rows)
}

/// One line on where updates stand.
fn status(app: &MusicApp) -> String {
    let updates = &app.updates;
    match &updates.state {
        State::Idle => "Updates come from GitHub Releases".into(),
        State::Checking => "Checking for updates…".into(),
        State::Current => "You have the newest version".into(),
        State::Available(release) => match updates.install.advice() {
            Some(advice) => format!("Version {} is available. {advice}.", release.version),
            None => format!("Version {} is available", release.version),
        },
        State::Downloading { version, .. } => format!(
            "Downloading version {version}… {}%",
            updates.percent().unwrap_or(0)
        ),
        State::Restarting(version) => format!("Restarting into version {version}…"),
        State::Failed(error) => error.clone(),
    }
}

/// Update and restart while one is on offer and this copy can replace
/// itself; the release page for one it can't; else Check now.
fn action(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let updates = &app.updates;
    match &updates.state {
        State::Checking | State::Downloading { .. } | State::Restarting(_) => {
            div().into_any_element()
        }
        State::Available(release) if updates.install.advice().is_some() => {
            let page = release.page.clone();
            widgets::pill_button("update-page", "Release page", None, Pill::Secondary, c)
                .on_click(move |_, _, cx| cx.open_url(&page))
                .into_any_element()
        }
        State::Available(_) => widgets::pill_button(
            "update-install",
            "Update and restart",
            None,
            Pill::Primary,
            c,
        )
        .on_click(cx.listener(|this, _, _, cx| this.install_update(cx)))
        .into_any_element(),
        _ => widgets::pill_button("update-check", "Check now", None, Pill::Secondary, c)
            .on_click(cx.listener(|this, _, _, cx| this.check_for_updates(true, cx)))
            .into_any_element(),
    }
}

/// In the top bar while a version is on offer: opens Settings.
pub fn pill(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    app.updates.available()?;
    let icon = widgets::icon(
        gpui_kit::assets::IconName::CircleArrowUp,
        crate::theme::size::ICON_SM,
        c.text,
    );
    Some(
        widgets::pill_button(
            "update-available",
            "Update available",
            Some(icon),
            Pill::Secondary,
            c,
        )
        .on_click(cx.listener(|this, _, window, cx| this.open_settings(true, window, cx)))
        .into_any_element(),
    )
}
