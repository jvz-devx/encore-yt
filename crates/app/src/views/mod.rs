//! The window's layout. Each slot is drawn by the module that owns it:
//!
//! - sidebar, top bar, page (M1: `sidebar`, `top_bar`, `page`)
//! - player bar, Up next, Now Playing, error strip (M2: `player`, `queue`,
//!   `now_playing`); the player bar is a view of its own, laid out by the
//!   shell (`visuals::shell`) in the room `player::space` leaves
//! - account chip, dialogs and Settings (M3: `account`, `settings`)
//! - overlays: Play anything, shortcuts, menus (M4: `overlays`, `menu`)
//! - Stage, equalizer, sleep timer (M6: `extras`)
//!
//! Shared recipes (icons, buttons, covers, skeletons) live in `widgets`;
//! colours, sizes and type in `crate::theme`. See `docs/gpui/DESIGN.md`.

mod account;
pub(crate) mod extras;
mod glide;
pub(crate) mod keyed;
mod menu;
mod now_playing;
mod overlays;
mod page;
mod player;
mod queue;
mod settings;
mod sidebar;
mod top_bar;
mod widgets;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::Run;

use crate::app::MusicApp;
use crate::theme::{self, Type, radius, size, space};
use widgets::Pill;

pub use player::layer::PlayerBar;

/// The keys of focused rows and cards, chip rows and sliders (M29).
pub fn bind_keys(cx: &mut App) {
    page::item_keys::bind_keys(cx);
    keyed::bind_keys(cx);
    extras::bind_eq_keys(cx);
}

pub fn root(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    // Stage replaces the whole window while it's open.
    if let Some(stage) = extras::stage(app, window, cx) {
        return stage;
    }
    if let Some(visualizer) = extras::visualizer(app, window, cx) {
        return visualizer;
    }
    let c = theme::colors(cx);
    let main = if app.player.now_playing {
        now_playing::now_playing(app, window, cx)
    } else {
        page::page(app, window, cx)
    };
    // The page panel: the brightest surface, inset from the window's base
    // with a large radius, so the sidebar and player bar frame it. Behind
    // Now Playing it is see-through: the effects layer paints it.
    let see_through = crate::visuals::fills_panel(app);
    let panel = v_flex()
        .flex_1()
        .h_full()
        .min_w_0()
        .mt(space::SM)
        .mr(space::SM)
        .mb(space::XS)
        .rounded(radius::LG)
        .when(!see_through, |panel| panel.bg(c.surface))
        .overflow_hidden()
        .child(top_bar::top_bar(app, window, cx))
        .children(error_strip(app, cx))
        .child(main);
    let root = v_flex()
        .key_context("Music")
        .track_focus(&app.focus)
        .size_full()
        .relative()
        // The window's base colour is painted under this by `visuals::shell`.
        .text_color(c.text)
        .type_body()
        // Decoded covers, held to a memory budget; those on screen stay.
        .image_cache(page::covers::root_cache(cx))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .items_start()
                .child(sidebar::sidebar(app, window, cx))
                .child(panel)
                .children(queue::panel(app, window, cx)),
        )
        // The player bar is its own view under these (`player::layer`).
        .child(player::space())
        .children(overlays::overlays(app, window, cx))
        // Dialogs and Settings (M3).
        .children(account::layer(app, window, cx));
    let root = crate::pages::on_actions(root, cx);
    let root = crate::playback::on_actions(root, cx);
    let root = crate::account::on_actions(root, cx);
    let root = crate::desktop::on_actions(root, cx);
    let root = crate::extras::on_actions(root, cx);
    root.into_any_element()
}

/// The newest error, under the top bar: what happened in plain words, the
/// technical detail behind Copy details, and Dismiss.
fn error_strip(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<impl IntoElement> {
    let error = app.error.clone()?;
    let (plain, detail) = crate::playback::split_error(&error);
    let c = theme::colors(cx);
    let small = |id: &'static str, label: &'static str, icon: Option<IconName>| {
        widgets::pill_button(
            id,
            label,
            icon.map(|i| widgets::icon(i, size::ICON_SM, c.text)),
            Pill::Secondary,
            &c,
        )
        .h(px(28.))
    };
    Some(
        h_flex()
            .mx(size::GUTTER)
            .mb(space::MD)
            .pl(space::MD)
            .pr(space::XS)
            .py(space::XS)
            .gap(space::MD)
            .rounded(radius::MD)
            .bg(c.danger_soft)
            .type_small()
            .child(widgets::icon(
                IconName::CircleAlert,
                size::ICON_SM,
                c.danger,
            ))
            .child(div().flex_1().min_w_0().line_clamp(2).child(plain))
            .children(detail.map(|_| {
                small("copy-error", "Copy details", Some(IconName::Copy))
                    .tooltip(widgets::tooltip("Copy the full error"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(error.clone()));
                    })
            }))
            .child(
                small("dismiss-error", "Dismiss", None).on_click(cx.listener(|this, _, _, cx| {
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
