//! M29: the keyboard on a page's song rows and cards. Each one is a tab
//! stop with a ring while the keyboard is on it: ↑/↓ move to the one
//! before or after, Enter plays it (its own click), and the Menu key or
//! Shift+F10 opens its menu under it. The page's own keys (Space, ←/→, the
//! letters) keep working.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

use crate::app::MusicApp;
use crate::theme::{Colors, radius};
use crate::views::{keyed, menu};

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
/// A row's ring sits inside its edge; a card has no fill of its own, so
/// while focused it takes the page's colour and the ring stands a little
/// outside it, clear of the cover.
pub fn hook(
    el: Stateful<Div>,
    key: &str,
    shelf: usize,
    item: usize,
    anchor: Anchor,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let store = bounds.clone();
    let key = key.to_string();
    let el = match anchor {
        Anchor::Row => keyed::ring_inside(el, c),
        Anchor::Card => {
            let (ring, page) = (c.focus_ring, c.surface);
            el.tab_index(0).focus_visible(move |s| {
                s.bg(page)
                    .rounded(radius::LG)
                    .shadow(vec![band(ring, 4.), band(page, 2.)])
            })
        }
    };
    el.key_context(CONTEXT)
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
        )
}

/// A solid band `spread` wide around an element.
fn band(color: Hsla, spread: f32) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(spread),
        inset: false,
    }
}
