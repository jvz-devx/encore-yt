//! The window: sidebar, page area and player bar.

mod page;
mod player;
mod sidebar;

use gpui_kit::component::{ActiveTheme, StyledExt, h_flex, v_flex};
use gpui_kit::*;
use ytfast::model::{Account, Run};

use crate::app::MusicApp;

pub fn root(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        // Decoded covers stay across frames while they're on screen.
        .image_cache(retain_all("covers"))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(sidebar::sidebar(app, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .h_full()
                        .min_w_0()
                        .child(top_bar(app, cx))
                        .children(error_strip(app, cx))
                        .child(page::page(app, window, cx)),
                ),
        )
        .child(player::player_bar(app, cx))
        .into_any_element()
}

fn top_bar(app: &MusicApp, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let theme = cx.theme();
    let account = match &app.account {
        Account::Checking => "Checking your account…".to_string(),
        Account::SignedIn { name, .. } => name.clone(),
        Account::SignedOut { .. } => "Signed out of YouTube Music".to_string(),
        Account::Unverified { .. } => "Offline".to_string(),
    };
    h_flex()
        .px_4()
        .py_3()
        .gap_3()
        .items_center()
        .child(
            div()
                .flex_1()
                .max_w(px(560.))
                .child(gpui_kit::component::input::Input::new(&app.search).cleanable(true)),
        )
        .child(div().flex_1())
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(account),
        )
}

fn error_strip(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<impl IntoElement> {
    let error = app.error.clone()?;
    let theme = cx.theme();
    Some(
        h_flex()
            .mx_4()
            .px_3()
            .py_2()
            .gap_3()
            .rounded(theme.radius)
            .bg(theme.danger.opacity(0.15))
            .text_sm()
            .child(div().flex_1().child(error))
            .child(
                div()
                    .id("dismiss-error")
                    .cursor_pointer()
                    .font_semibold()
                    .child("Dismiss")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    })),
            ),
    )
}

/// A subtitle as one line of text: "Song • Artist • 3:45".
pub fn runs_text(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// "3:45", or "1:02:03" past an hour.
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total / 60) % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
