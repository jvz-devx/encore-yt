//! The context menu (DESIGN.md "Panels"): an `overlay` panel at the
//! pointer, kept inside the window, in sections split by hairlines. One
//! highlight follows the pointer and the arrows alike. Also the ⋮ buttons
//! and right-click hooks rows and cards call into (`desktop::menu` holds
//! the state and what the entries do).

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Item, Page};

use super::widgets;
use crate::app::MusicApp;
use crate::desktop::menu::{
    self, CONTEXT, Entry, MenuChoose, MenuClose, MenuDown, MenuUp, Place, Subject,
};
use crate::theme::motion::MotionExt as _;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};

/// The menu's width.
const WIDTH: Pixels = px(248.);
/// An entry's height.
const ENTRY: Pixels = px(36.);

/// The open menu over everything, with a clear backdrop that closes it on
/// a click or scroll elsewhere.
pub fn layer(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    let open = app.desktop.layers.menu.as_ref()?;
    let c = theme::colors(cx);
    let entries = menu::entries(app, &open.subject);
    let panel = v_flex()
        .id("context-menu")
        .key_context(CONTEXT)
        .track_focus(&open.focus)
        .w(WIDTH)
        .p(space::XS)
        .rounded(radius::LG)
        .bg(c.overlay)
        .shadow(elevation::high(&c))
        .text_color(c.text)
        .on_action(cx.listener(|this, _: &MenuUp, _, cx| this.move_in_menu(-1, cx)))
        .on_action(cx.listener(|this, _: &MenuDown, _, cx| this.move_in_menu(1, cx)))
        .on_action(
            cx.listener(|this, _: &MenuChoose, window, cx| this.choose_in_menu(None, window, cx)),
        )
        .on_action(cx.listener(|this, _: &MenuClose, window, cx| this.close_menu(window, cx)))
        // Presses inside stay inside.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .children(rows(&entries, open.selected, &c, cx))
        .with_motion(
            SharedString::from(format!("menu-{}", open.serial)),
            motion::Kind::Menus,
            motion::FAST,
            |el, t| el.opacity(t).top(px(-4.) * (1. - t)),
        );
    let close = cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_menu(window, cx));
    let close_right =
        cx.listener(|this, _: &MouseDownEvent, window, cx| this.close_menu(window, cx));
    let backdrop = div()
        .id("menu-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(MouseButton::Left, close)
        .on_mouse_down(MouseButton::Right, close_right)
        .on_scroll_wheel(cx.listener(|this, _, window, cx| this.close_menu(window, cx)))
        .child(
            anchored()
                .position(open.at)
                .child(div().relative().child(panel)),
        );
    Some(backdrop.into_any_element())
}

/// The entries, with a hairline between sections.
fn rows(
    entries: &[Entry],
    selected: Option<usize>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        if i > 0 && entries[i - 1].section != e.section {
            out.push(
                div()
                    .h(px(1.))
                    .mx(space::SM)
                    .my(space::XS)
                    .bg(c.hairline)
                    .into_any_element(),
            );
        }
        out.push(row(i, e, selected == Some(i), c, cx).into_any_element());
    }
    out
}

fn row(
    i: usize,
    e: &Entry,
    selected: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    h_flex()
        .id(("menu-entry", i))
        .debug_selector(|| format!("menu-entry:{}", e.label))
        .h(ENTRY)
        .px(space::MD)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .type_label()
        .when(selected, |s| s.bg(c.selected))
        .active(|s| s.bg(c.pressed))
        .child(widgets::icon(
            e.icon,
            size::ICON_SM,
            if selected { c.text } else { c.text_muted },
        ))
        .child(e.label)
        // Moving the pointer (not a menu opening under it) highlights.
        .on_mouse_move(
            cx.listener(move |this, _: &MouseMoveEvent, _, cx| this.hover_in_menu(i, cx)),
        )
        .on_click(cx.listener(move |this, _, window, cx| this.choose_in_menu(Some(i), window, cx)))
}

/// The ⋮ button that opens a row's menu (36 px, ghost).
pub fn dots(id: impl Into<ElementId>, c: &Colors) -> Stateful<Div> {
    widgets::icon_button(
        id,
        widgets::icon(IconName::EllipsisVertical, size::ICON, c.text_muted),
        c,
    )
}

/// The ⋮ on a cover: a dark disc with a light glyph, the same in both
/// looks because it sits on the art.
pub fn cover_dots(id: impl Into<ElementId>, c: &Colors) -> Stateful<Div> {
    h_flex()
        .id(id)
        .size(size::ICON_BUTTON)
        .justify_center()
        .rounded(radius::FULL)
        .bg(c.scrim)
        .cursor_pointer()
        .hover(|s| s.bg(c.shadow))
        .child(widgets::icon(
            IconName::EllipsisVertical,
            size::ICON,
            c.on_media,
        ))
}

/// Whether an item has a menu (music: songs, albums, playlists, artists).
pub fn has_menu(item: &Item) -> bool {
    Subject::of_item(item, Place::List).is_some()
}

/// Item `item` of shelf `shelf` on page `key`, looked up when it's clicked.
fn find(app: &MusicApp, key: &str, shelf: usize, item: usize) -> Option<Item> {
    let page: &Page = app.pages.states.get(key)?.page.as_ref()?;
    page.shelves.get(shelf)?.items.get(item).cloned()
}

fn open_item(
    app: &mut MusicApp,
    key: &str,
    shelf: usize,
    item: usize,
    at: Point<Pixels>,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) {
    let subject =
        find(app, key, shelf, item).and_then(|i| Subject::of_item(&i, place(app, key, &i)));
    if let Some(subject) = subject {
        app.open_menu(subject, at, window, cx);
    }
}

/// Where an item of page `key` sits: an entry of the account's own playlist
/// offers Remove from playlist (M3).
fn place(app: &MusicApp, key: &str, item: &Item) -> Place {
    let own = app
        .pages
        .states
        .get(key)
        .and_then(|s| s.page.as_ref()?.header.as_ref()?.editable.clone());
    let entry = item.track.as_ref().and_then(|t| t.set_video_id.clone());
    match (own, entry) {
        (Some(playlist_id), Some(set_video_id)) => Place::Own {
            playlist_id,
            set_video_id,
        },
        _ => Place::List,
    }
}

/// Right-click on a page's row or card: its menu at the pointer.
pub fn on_item_right_click(
    key: &str,
    shelf: usize,
    item: usize,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    let key = key.to_string();
    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        open_item(this, &key, shelf, item, event.position, window, cx);
    })
}

/// A ⋮ click on a page's row or card: its menu under the pointer.
pub fn on_item_dots(
    key: &str,
    shelf: usize,
    item: usize,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let key = key.to_string();
    cx.listener(move |this, event: &ClickEvent, window, cx| {
        cx.stop_propagation();
        open_item(this, &key, shelf, item, event.position(), window, cx);
    })
}

/// Up next row `i`: its song's menu at `at`, with Remove from queue.
pub fn open_queued(
    app: &mut MusicApp,
    i: usize,
    at: Point<Pixels>,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) {
    if let Some(track) = app.player.queue.get(i).cloned() {
        let subject = Subject::Song {
            track,
            place: Place::UpNext(i),
        };
        app.open_menu(subject, at, window, cx);
    }
}

/// A song row's trailing cell: its length, which gives way to ⋮ while the
/// row (`group`) is under the pointer.
pub fn row_trailing(
    id: SharedString,
    length: Option<String>,
    group: &'static str,
    on_dots: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    c: &Colors,
) -> impl IntoElement {
    div()
        .relative()
        .flex_none()
        .min_w(size::ICON_BUTTON)
        .h(size::ICON_BUTTON)
        .children(length.map(|d| {
            h_flex()
                .h_full()
                .pl(space::SM)
                .justify_end()
                .type_small()
                .tabular()
                .text_color(c.text_faint)
                .group_hover(group, |s| s.opacity(0.))
                .child(d)
        }))
        .child(
            dots(id, c)
                .absolute()
                .top_0()
                .right_0()
                .opacity(0.)
                .group_hover(group, |s| s.opacity(1.))
                .on_click(on_dots),
        )
}
