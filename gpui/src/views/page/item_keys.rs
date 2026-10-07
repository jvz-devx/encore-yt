//! M29: the keyboard on a page's song rows and cards. Each one is a tab
//! stop with a ring while the keyboard is on it ([`ItemFocus`]): ↑/↓ move to the one
//! before or after, Enter plays it (its own click), and the Menu key or
//! Shift+F10 opens its menu under it. The page's own keys (Space, ←/→, the
//! letters) keep working.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::{Colors, radius};
use crate::views::menu;

actions!(music_item, [ItemMenu, ItemUp, ItemDown]);

/// The key context of a focused row or card.
pub const CONTEXT: &str = "MusicItem";

pub fn bind_keys(cx: &mut App) {
    let item = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("shift-f10", ItemMenu, item),
        KeyBinding::new("menu", ItemMenu, item),
        KeyBinding::new("up", ItemUp, item),
        KeyBinding::new("down", ItemDown, item),
    ]);
}

/// Where the menu opens: a row's at its right end, as its ⋮ would; a
/// card's under its left edge.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    Row,
    Card,
}

/// Makes row or card `item` of shelf `shelf` on page `key` a tab stop.
pub fn hook(
    el: Stateful<Div>,
    key: &str,
    shelf: usize,
    item: usize,
    anchor: Anchor,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> ItemFocus {
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let store = bounds.clone();
    let id = SharedString::from(format!("item-focus:{key}:{shelf}:{item}"));
    let key = key.to_string();
    let el = el
        .key_context(CONTEXT)
        .on_action(|_: &ItemUp, window, cx| window.focus_prev(cx))
        .on_action(|_: &ItemDown, window, cx| window.focus_next(cx))
        .on_action(cx.listener(move |this, _: &ItemMenu, window, cx| {
            let b = bounds.get();
            let at = match anchor {
                Anchor::Row => point(b.right() - menu::WIDTH, b.bottom()),
                Anchor::Card => point(b.left(), b.bottom() + px(4.)),
            };
            menu::open_item(this, &key, shelf, item, at, window, cx);
        }))
        .child(
            canvas(move |b, _, _| store.set(b), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        );
    ItemFocus {
        el,
        id,
        radius: match anchor {
            Anchor::Row => radius::MD,
            Anchor::Card => radius::MD,
        },
        outset: match anchor {
            Anchor::Row => px(0.),
            Anchor::Card => px(5.),
        },
        ring: c.focus_ring,
    }
}

/// A row or card with its focus: the ring is a line drawn over it while the
/// keyboard is on it: on a row's edge, a little outside a card.
#[derive(IntoElement)]
pub struct ItemFocus {
    el: Stateful<Div>,
    id: SharedString,
    radius: Pixels,
    /// How far the ring stands outside: a card's clear of its cover and
    /// title (the carousel leaves room for it), a row's on its edge.
    outset: Pixels,
    ring: Hsla,
}

impl RenderOnce for ItemFocus {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // A handle of our own is a tab stop only if it says so itself.
        let focus = window
            .use_keyed_state(self.id, cx, |_, cx| cx.focus_handle().tab_stop(true))
            .read(cx)
            .clone();
        let shown = focus.is_focused(window) && window.last_input_was_keyboard();
        let (radius, ring, outset) = (self.radius, self.ring, self.outset);
        self.el.track_focus(&focus).when(shown, |el| {
            el.child(
                div()
                    .absolute()
                    .top(-outset)
                    .left(-outset)
                    .right(-outset)
                    .bottom(-outset)
                    .rounded(radius + outset)
                    .border_2()
                    .border_color(ring),
            )
        })
    }
}
