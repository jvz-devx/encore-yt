//! M29: the keyboard on a page's song rows and cards. Each one is a tab
//! stop with a ring while the keyboard is on it ([`ItemFocus`]): ↑/↓ move to the one
//! before or after, Enter plays it (its own click), and the Menu key or
//! Shift+F10 opens its menu under it. The page's own keys (Space, ←/→, the
//! letters) keep working.
//!
//! An item the keyboard moves to scrolls into view: its carousel glides
//! sideways (as its arrows do) and the page scrolls up or down at once,
//! each just far enough to show it with a little room around it.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Ctx, shelf as shelves};
use crate::app::MusicApp;
use crate::theme::{radius, space};
use crate::views::menu;

actions!(music_item, [ItemMenu, ItemUp, ItemDown]);

/// The key context of a focused row or card.
pub const CONTEXT: &str = "MusicItem";

/// The room kept above and below an item the page scrolls to.
const PAGE_MARGIN: Pixels = space::LG;
/// The room kept beside a card a carousel scrolls to: the carousel's own
/// padding, so the first and last cards land where they rest.
const CAROUSEL_MARGIN: Pixels = space::SM;

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

/// Makes row or card `item` of shelf `shelf` on the page a tab stop.
pub fn hook(
    el: Stateful<Div>,
    ctx: &Ctx,
    shelf: usize,
    item: usize,
    anchor: Anchor,
    cx: &mut Context<MusicApp>,
) -> ItemFocus {
    let bounds = Rc::new(Cell::new(Bounds::default()));
    let menu_at = bounds.clone();
    let key = ctx.key.clone();
    let id = SharedString::from(format!("item-focus:{key}:{shelf}:{item}"));
    let reveal = Reveal {
        app: cx.entity().downgrade(),
        shelf: (key.clone(), shelf),
        carousel: ctx.carousel.clone(),
    };
    let el = el
        .key_context(CONTEXT)
        .on_action(|_: &ItemUp, window, cx| window.focus_prev(cx))
        .on_action(|_: &ItemDown, window, cx| window.focus_next(cx))
        .on_action(cx.listener(move |this, _: &ItemMenu, window, cx| {
            let b = menu_at.get();
            let at = match anchor {
                Anchor::Row => point(b.right() - menu::WIDTH, b.bottom()),
                Anchor::Card => point(b.left(), b.bottom() + px(4.)),
            };
            menu::open_item(this, &key, shelf, item, at, window, cx);
        }));
    ItemFocus {
        el,
        id,
        bounds,
        reveal,
        radius: radius::MD,
        outset: match anchor {
            Anchor::Row => px(0.),
            Anchor::Card => px(5.),
        },
        ring: ctx.c.focus_ring,
    }
}

/// What scrolls to show an item: the page's list (by GPUI's autoscroll)
/// and the item's carousel, if it is in one.
struct Reveal {
    app: WeakEntity<MusicApp>,
    /// The page key and shelf, which name the carousel's glide.
    shelf: (String, usize),
    carousel: Option<ScrollHandle>,
}

impl Reveal {
    /// Scrolls to `item` (where it was laid out this frame). Runs while
    /// the page's list lays out, so its request is answered in this frame.
    fn show(self, item: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        window.request_autoscroll(Bounds::from_corners(
            point(item.left(), item.top() - PAGE_MARGIN),
            point(item.right(), item.bottom() + PAGE_MARGIN),
        ));
        let Some(scroll) = self.carousel else {
            return;
        };
        let view = scroll.bounds();
        let (left, right) = (
            view.left() + CAROUSEL_MARGIN,
            view.right() - CAROUSEL_MARGIN,
        );
        let from = -scroll.offset().x;
        let to = if item.left() < left {
            from - (left - item.left())
        } else if item.right() > right {
            from + (item.right() - right)
        } else {
            return;
        };
        let (app, shelf) = (self.app, self.shelf);
        cx.defer(move |cx| {
            app.update(cx, |app, cx| shelves::glide(app, shelf, scroll, to, cx))
                .ok();
        });
    }
}

/// A tab stop's own handle, and whether it has had the focus since it
/// last didn't: an item scrolls into view once each time the keyboard
/// arrives on it, not again when a key is pressed on it later.
struct ItemState {
    focus: FocusHandle,
    arrived: Rc<Cell<bool>>,
}

/// A row or card with its focus: the ring is a line drawn over it while the
/// keyboard is on it: on a row's edge, a little outside a card.
#[derive(IntoElement)]
pub struct ItemFocus {
    el: Stateful<Div>,
    id: SharedString,
    /// Where the item is drawn, for its menu and for scrolling to it.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    reveal: Reveal,
    radius: Pixels,
    /// How far the ring stands outside: a card's clear of its cover and
    /// title (the carousel leaves room for it), a row's on its edge.
    outset: Pixels,
    ring: Hsla,
}

impl RenderOnce for ItemFocus {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // A handle of our own is a tab stop only if it says so itself.
        let state = window.use_keyed_state(self.id, cx, |_, cx| ItemState {
            focus: cx.focus_handle().tab_stop(true),
            arrived: Rc::default(),
        });
        let ItemState { focus, arrived } = state.read(cx);
        let (focus, arrived) = (focus.clone(), arrived.clone());
        let focused = focus.is_focused(window);
        if !focused {
            arrived.set(false);
        }
        let shown = focused && window.last_input_was_keyboard();
        let (radius, ring, outset) = (self.radius, self.ring, self.outset);
        let (bounds, reveal) = (self.bounds, self.reveal);
        // Laid out: remember where, and scroll to it if the keyboard has
        // just arrived (a click focuses it too, but it is in view then).
        let laid_out = move |b: Bounds<Pixels>, window: &mut Window, cx: &mut App| {
            bounds.set(b);
            if focused && !arrived.replace(true) && shown {
                reveal.show(b, window, cx);
            }
        };
        self.el
            .track_focus(&focus)
            .child(canvas(laid_out, |_, _, _, _| {}).absolute().inset_0())
            .when(shown, |el| {
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
