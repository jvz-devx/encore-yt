//! Signed out: Explore's shortcuts where the playlists would be, Recently
//! played, and a quiet line saying where the library went.

use gpui_kit::assets::IconName;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Account, Target};

use super::library::recent;
use crate::app::MusicApp;
use crate::nav::View;
use crate::theme::{Colors, Type, space};

pub fn signed_out(app: &MusicApp, rail: bool, c: &Colors, cx: &mut Context<MusicApp>) -> Div {
    let shortcuts: Vec<AnyElement> = app
        .explore_shortcuts()
        .into_iter()
        .map(|(label, target)| {
            let active = matches!((&app.pages.view, &target),
                (View::Page(Target::Browse { id: open, .. }), Target::Browse { id, .. }) if open == id);
            super::item(label.into(), icon(&target), active, rail, c)
                .on_click(cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)))
                .into_any_element()
        })
        .collect();
    // Offline is not signed out: only a checked sign-out says so.
    let hint = matches!(app.account.account, Account::SignedOut { .. }) && !rail;
    v_flex()
        .gap(space::XS)
        .children(shortcuts)
        .child(recent(app, rail, c, cx))
        .when(hint, |s| {
            s.child(
                div()
                    .mt(space::XL)
                    .px(space::MD)
                    .type_small()
                    .text_color(c.text_faint)
                    .child("Sign in to see your library"),
            )
        })
}

fn icon(target: &Target) -> IconName {
    match target {
        Target::Browse { id, .. } if id == "FEmusic_new_releases" => IconName::Sparkles,
        Target::Browse { id, .. } if id == "FEmusic_charts" => IconName::TrendingUp,
        Target::Browse { id, .. } if id == "FEmusic_moods_and_genres" => IconName::Shapes,
        _ => IconName::Compass,
    }
}
