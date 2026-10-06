//! The window's layout. Each slot is drawn by the module that owns it:
//!
//! - sidebar, top bar, page (M1: `sidebar`, `top_bar`, `page`)
//! - player bar, Up next, Now Playing, error strip (M2: `player`, `queue`,
//!   `now_playing`)
//! - account chip and dialogs (M3: `account`)
//! - overlays: Play anything, shortcuts, menus (M4: `overlays`)
//! - Stage, equalizer, sleep timer (M6: `extras`)
//!
//! Shared helpers for text and colours live here and in `theme`.

mod account;
mod extras;
mod now_playing;
mod overlays;
mod page;
mod player;
mod queue;
mod sidebar;
mod top_bar;

use gpui_kit::component::{ActiveTheme, StyledExt, h_flex, v_flex};
use gpui_kit::*;
use ytfast::model::Run;

use crate::app::MusicApp;

pub fn root(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    // Stage replaces the whole window while it's open.
    if let Some(stage) = extras::stage(app, window, cx) {
        return stage;
    }
    let (background, foreground) = (cx.theme().background, cx.theme().foreground);
    let main = if app.player.now_playing {
        now_playing::now_playing(app, window, cx)
    } else {
        page::page(app, window, cx)
    };
    let root = v_flex()
        .key_context("Music")
        .track_focus(&app.focus)
        .size_full()
        .relative()
        .bg(background)
        .text_color(foreground)
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
                        .child(top_bar::top_bar(app, window, cx))
                        .children(error_strip(app, cx))
                        .child(main),
                )
                .children(queue::panel(app, window, cx)),
        )
        .child(player::player_bar(app, cx))
        .children(overlays::overlays(app, window, cx));
    let root = crate::pages::on_actions(root, cx);
    let root = crate::playback::on_actions(root, cx);
    let root = crate::account::on_actions(root, cx);
    let root = crate::desktop::on_actions(root, cx);
    let root = crate::extras::on_actions(root, cx);
    root.into_any_element()
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
