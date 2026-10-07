//! The keyboard in Settings' scrolling body (M33): Tab and the arrows can
//! land on a control below or above the fold, and its ring would be out of
//! sight. Every row of a section is wrapped in [`wrap`], which knows when
//! the keyboard has just arrived inside it and scrolls the body just far
//! enough to show it, with a little room around it. A click doesn't: the
//! thing clicked is in view already.
//!
//! GPUI keeps the kit controls' focus handles to itself and its scroll
//! containers don't follow focus, so the row (not the control) is the unit:
//! it has a handle of its own that only asks "is the focus inside me?".

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::space;

/// The room kept above and below the row the body scrolls to.
const MARGIN: Pixels = space::LG;

/// Scrolling up to within this of the top goes all the way.
const TOP_SNAP: Pixels = space::XXXL;

thread_local! {
    /// The body being laid out: its scroll handle, and the number of rows
    /// wrapped so far (their identity, stable from frame to frame).
    static BODY: RefCell<Option<ScrollHandle>> = const { RefCell::new(None) };
    static NEXT: Cell<usize> = const { Cell::new(0) };
    static LAST_ID: RefCell<Option<SharedString>> = const { RefCell::new(None) };
}

/// Starts a frame of Settings: rows wrapped from here on scroll `body`.
pub fn begin(body: &ScrollHandle) {
    BODY.with(|b| *b.borrow_mut() = Some(body.clone()));
    NEXT.with(|n| n.set(0));
}

/// The body's handle for the panel to track. A different body (another
/// category) starts at the top.
pub fn track(id: &SharedString) -> ScrollHandle {
    let handle = BODY.with(|b| b.borrow().clone()).unwrap_or_default();
    LAST_ID.with(|last| {
        let mut last = last.borrow_mut();
        if last.as_ref() != Some(id) {
            handle.set_offset(point(px(0.), px(0.)));
            *last = Some(id.clone());
        }
    });
    handle
}

/// A row that scrolls the body to itself when the keyboard arrives in it.
pub fn wrap(row: AnyElement) -> AnyElement {
    let Some(scroll) = BODY.with(|b| b.borrow().clone()) else {
        return row;
    };
    let n = NEXT.with(|n| n.replace(n.get() + 1));
    Row {
        id: format!("settings-row:{n}").into(),
        row,
        scroll,
    }
    .into_any_element()
}

#[derive(IntoElement)]
struct Row {
    id: SharedString,
    row: AnyElement,
    scroll: ScrollHandle,
}

struct State {
    focus: FocusHandle,
    arrived: Rc<Cell<bool>>,
}

impl RenderOnce for Row {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state(self.id, cx, |_, cx| State {
            focus: cx.focus_handle(),
            arrived: Rc::default(),
        });
        let State { focus, arrived } = state.read(cx);
        let (focus, arrived) = (focus.clone(), arrived.clone());
        let inside = focus.contains_focused(window, cx);
        if !inside {
            arrived.set(false);
        }
        let keyboard = inside && window.last_input_was_keyboard();
        let scroll = self.scroll;
        let laid_out = move |row: Bounds<Pixels>, window: &mut Window, _: &mut App| {
            if keyboard && !arrived.replace(true) && show(&scroll, row) {
                window.refresh();
            }
        };
        v_flex()
            .w_full()
            .track_focus(&focus)
            .when(inside, |row| {
                row.debug_selector(|| "settings-focused-row".into())
            })
            .child(canvas(laid_out, |_, _, _, _| {}).absolute().inset_0())
            .child(self.row)
    }
}

/// Moves the body so `row` is inside it; false when it already was.
fn show(scroll: &ScrollHandle, row: Bounds<Pixels>) -> bool {
    let view = scroll.bounds();
    if view.size.height <= px(0.) {
        return false;
    }
    let (top, bottom) = (row.top() - MARGIN, row.bottom() + MARGIN);
    let shift = if top < view.top() {
        view.top() - top
    } else if bottom > view.bottom() {
        // A row taller than the view keeps its top in sight.
        -(bottom - view.bottom()).min(row.top() - view.top())
    } else {
        return false;
    };
    let mut offset = scroll.offset();
    let max = scroll.max_offset().y;
    offset.y = (offset.y + shift).clamp(-max, px(0.));
    // Near the top, show the section's name above its first row too.
    if shift > px(0.) && offset.y > -TOP_SNAP {
        offset.y = px(0.);
    }
    scroll.set_offset(offset);
    true
}
