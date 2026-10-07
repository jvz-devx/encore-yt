//! The context menu (DESIGN.md "Panels"): a panel at the pointer (or under
//! the item the keyboard opened it from), kept inside the window, in
//! sections split by hairlines. One highlight follows the pointer and the
//! keys alike; Add to playlist opens a submenu beside it. Also the ⋮
//! buttons and right-click hooks rows and cards call into
//! (`desktop::menu` holds the state and what the entries do).

use encore_core::model::{Item, Page};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets;
use crate::app::MusicApp;
use crate::desktop::lists::List;
use crate::desktop::menu::{
    self, CONTEXT, Entry, MenuChoose, MenuClose, MenuDown, MenuFirst, MenuIn, MenuLast, MenuOut,
    MenuUp, Place, Subject,
};
use crate::desktop::menu_keys::typed_letter;
use crate::theme::motion::MotionExt as _;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};

/// The menu's width.
pub const WIDTH: Pixels = px(248.);
/// An entry's height.
const ENTRY: Pixels = px(36.);
/// A hairline between sections, with its room.
const LINE: Pixels = px(9.);
/// Kept clear at the window's edges.
const EDGE: Pixels = px(8.);

/// One drawn entry of the menu or its submenu.
struct Shown {
    icon: IconName,
    label: SharedString,
    section: u8,
    sub: bool,
}

/// The open menu over everything, with a clear backdrop that closes it on
/// a click or scroll elsewhere.
pub fn layer(app: &MusicApp, window: &Window, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    let open = app.desktop.layers.menu.as_ref()?;
    let c = theme::colors(cx);
    let shown: Vec<Shown> = menu::entries(app, &open.subject)
        .iter()
        .map(|e: &Entry| Shown {
            icon: e.icon,
            label: e.label.into(),
            section: e.section,
            sub: e.opens_sub(),
        })
        .collect();
    let viewport = window.viewport_size();
    let at = fit(open.at, size(WIDTH, height(&shown)), viewport);
    let panel = v_flex()
        .id("context-menu")
        .debug_selector(|| "context-menu".into())
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
        .on_action(cx.listener(|this, _: &MenuFirst, _, cx| this.menu_end(false, cx)))
        .on_action(cx.listener(|this, _: &MenuLast, _, cx| this.menu_end(true, cx)))
        .on_action(cx.listener(|this, _: &MenuIn, _, cx| this.menu_in(cx)))
        .on_action(cx.listener(|this, _: &MenuOut, _, cx| {
            this.close_sub(cx);
        }))
        .on_action(
            cx.listener(|this, _: &MenuChoose, window, cx| this.choose_in_menu(None, window, cx)),
        )
        .on_action(cx.listener(|this, _: &MenuClose, window, cx| this.escape_menu(window, cx)))
        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
            if let Some(letter) = typed_letter(e)
                && this.menu_letter(letter, cx)
            {
                cx.stop_propagation();
            }
        }))
        // Presses inside stay inside.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .children(rows(
            &shown,
            open.selected,
            "menu-entry",
            &c,
            cx,
            |i| Box::new(move |this, cx| this.hover_in_menu(i, cx)),
            |i| Box::new(move |this, window, cx| this.choose_in_menu(Some(i), window, cx)),
        ))
        .with_motion(
            SharedString::from(format!("menu-{}", open.serial)),
            motion::Kind::Menus,
            motion::FAST,
            |el, t| el.opacity(t).top(px(-4.) * (1. - t)),
        );
    let sub = open
        .sub
        .as_ref()
        .map(|s| submenu(app, &shown, s, at, viewport, &c, cx));
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
            div()
                .absolute()
                .left(at.x)
                .top(at.y)
                .child(div().relative().child(panel)),
        )
        .children(sub);
    Some(backdrop.into_any_element())
}

/// Add to playlist's submenu beside its entry: right of the menu, or left
/// where the window ends.
fn submenu(
    app: &MusicApp,
    main: &[Shown],
    open: &crate::desktop::submenu::Sub,
    at: Point<Pixels>,
    viewport: Size<Pixels>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let shown: Vec<Shown> = crate::desktop::submenu::sub_entries(app)
        .into_iter()
        .map(|e| Shown {
            icon: e.icon,
            label: e.label,
            section: e.section,
            sub: false,
        })
        .collect();
    let h = height(&shown);
    let fits_right = at.x + WIDTH * 2. - space::XS + EDGE <= viewport.width;
    let x = if fits_right {
        at.x + WIDTH - space::XS
    } else {
        at.x - WIDTH + space::XS
    };
    let y = (at.y + offset(main, open.parent))
        .min(viewport.height - EDGE - h)
        .max(EDGE);
    let panel = v_flex()
        .id("context-submenu")
        .debug_selector(|| "context-submenu".into())
        .w(WIDTH)
        .p(space::XS)
        .rounded(radius::LG)
        .bg(c.overlay)
        .shadow(elevation::high(c))
        .text_color(c.text)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .children(rows(
            &shown,
            open.selected,
            "submenu-entry",
            c,
            cx,
            |i| Box::new(move |this, cx| this.hover_in_sub(i, cx)),
            |i| Box::new(move |this, window, cx| this.choose_in_sub(Some(i), window, cx)),
        ));
    div()
        .absolute()
        .left(x)
        .top(y)
        .child(panel)
        .into_any_element()
}

/// Where a panel `panel` big opens for a press at `at`: below and right of
/// it, flipped where the window ends.
fn fit(at: Point<Pixels>, panel: Size<Pixels>, viewport: Size<Pixels>) -> Point<Pixels> {
    // Flips to the other side of `at` when it doesn't fit, and always
    // stays inside the window (an anchor from the keyboard can lie off
    // screen, e.g. a card scrolled out of a shelf).
    let flip = |at: Pixels, len: Pixels, room: Pixels| {
        let at = if at + len + EDGE > room { at - len } else { at };
        at.min(room - len - EDGE).max(EDGE)
    };
    point(
        flip(at.x, panel.width, viewport.width),
        flip(at.y, panel.height, viewport.height),
    )
}

/// A panel's height: its padding, entries and lines.
fn height(shown: &[Shown]) -> Pixels {
    space::XS * 2. + offset(shown, shown.len()) - space::XS
}

/// How far entry `i`'s top sits below the panel's top.
fn offset(shown: &[Shown], i: usize) -> Pixels {
    let lines = (1..i.min(shown.len()))
        .filter(|&k| shown[k - 1].section != shown[k].section)
        .count();
    space::XS + ENTRY * i as f32 + LINE * lines as f32
}

type Hover = Box<dyn Fn(&mut MusicApp, &mut Context<MusicApp>)>;
type Choose = Box<dyn Fn(&mut MusicApp, &mut Window, &mut Context<MusicApp>)>;

/// The entries, with a hairline between sections.
fn rows(
    shown: &[Shown],
    selected: Option<usize>,
    name: &'static str,
    c: &Colors,
    cx: &mut Context<MusicApp>,
    hover: impl Fn(usize) -> Hover,
    choose: impl Fn(usize) -> Choose,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (i, e) in shown.iter().enumerate() {
        if i > 0 && shown[i - 1].section != e.section {
            out.push(
                div()
                    .h(px(1.))
                    .mx(space::SM)
                    .my(space::XS)
                    .bg(c.hairline)
                    .into_any_element(),
            );
        }
        let (h, ch) = (hover(i), choose(i));
        let el = row(name, i, e, selected == Some(i), c)
            // Moving the pointer (not a menu opening under it) highlights.
            .on_mouse_move(cx.listener(move |this, _: &MouseMoveEvent, _, cx| h(this, cx)))
            .on_click(cx.listener(move |this, _, window, cx| ch(this, window, cx)));
        out.push(el.into_any_element());
    }
    out
}

fn row(name: &'static str, i: usize, e: &Shown, selected: bool, c: &Colors) -> Stateful<Div> {
    let label = e.label.clone();
    h_flex()
        .id((name, i))
        .debug_selector(move || format!("{name}:{label}"))
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
        .child(div().flex_1().min_w_0().truncate().child(e.label.clone()))
        .when(e.sub, |r| {
            r.child(widgets::icon(
                IconName::ChevronRight,
                size::ICON_SM,
                c.text_muted,
            ))
        })
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

/// Opens the menu of item `item` of shelf `shelf` on page `key` at `at`.
pub fn open_item(
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

/// The keys of a menu-like list (`desktop::lists`) on its panel: the
/// `MusicMenu` context, the arrows, Home/End, type-ahead, Enter or Space,
/// and Esc (`close`).
pub fn list_keys(
    el: Stateful<Div>,
    list: List,
    focus: &FocusHandle,
    close: fn(&mut MusicApp, &mut Window, &mut Context<MusicApp>),
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    el.key_context(CONTEXT)
        .track_focus(focus)
        .on_action(cx.listener(move |this, _: &MenuUp, _, cx| this.list_move(list, -1, cx)))
        .on_action(cx.listener(move |this, _: &MenuDown, _, cx| this.list_move(list, 1, cx)))
        .on_action(cx.listener(move |this, _: &MenuFirst, _, cx| this.list_ends(list, false, cx)))
        .on_action(cx.listener(move |this, _: &MenuLast, _, cx| this.list_ends(list, true, cx)))
        .on_action(cx.listener(move |this, _: &MenuChoose, window, cx| {
            this.list_choose(list, None, window, cx)
        }))
        .on_action(cx.listener(move |this, _: &MenuClose, window, cx| close(this, window, cx)))
        .on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
            if let Some(letter) = typed_letter(e)
                && this.list_letter(list, letter, cx)
            {
                cx.stop_propagation();
            }
        }))
}

/// An entry of such a list: a click chooses it.
pub fn list_entry(
    el: Stateful<Div>,
    list: List,
    i: usize,
    selected: Option<usize>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    list_highlight(el, list, i, selected, c, cx).on_click(
        cx.listener(move |this, _, window, cx| this.list_choose(list, Some(i), window, cx)),
    )
}

/// The highlight of a list's entry `i`: the list's, set by the pointer
/// and the keys alike (so the entry has no hover style of its own).
pub fn list_highlight(
    el: Stateful<Div>,
    list: List,
    i: usize,
    selected: Option<usize>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    el.when(selected == Some(i), |s| s.bg(c.selected))
        .on_mouse_move(
            cx.listener(move |this, _: &MouseMoveEvent, _, cx| this.list_hover(list, i, cx)),
        )
}
