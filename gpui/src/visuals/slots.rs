//! Where Now Playing wants its effects. Now Playing (inside the cached app
//! view) lays out empty boxes with [`slot`]; their bounds land here during
//! the app's prepaint, and the effects and flight layers read them when they
//! paint, in the same frame or, while the app view is reused, from before.

use std::cell::{Cell, RefCell};

use gpui_kit::*;

#[derive(Clone, Copy, Debug)]
pub enum Slot {
    /// Now Playing's area of the page panel (below the top bar).
    Page,
    /// The large cover.
    Cover,
    /// The strip for the spectrum.
    Spectrum,
}

/// The cover's image URLs at the sizes the app loads them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Covers {
    /// At the player bar's size: loaded whenever a song plays.
    pub small: Option<SharedString>,
    /// At Now Playing's size.
    pub large: Option<SharedString>,
}

#[derive(Default)]
pub(super) struct Slots {
    page: Cell<Option<Bounds<Pixels>>>,
    cover: Cell<Option<Bounds<Pixels>>>,
    spectrum: Cell<Option<Bounds<Pixels>>>,
    covers: RefCell<Covers>,
}

impl Global for Slots {}

impl Slots {
    fn cell(&self, slot: Slot) -> &Cell<Option<Bounds<Pixels>>> {
        match slot {
            Slot::Page => &self.page,
            Slot::Cover => &self.cover,
            Slot::Spectrum => &self.spectrum,
        }
    }

    pub fn get(cx: &App, slot: Slot) -> Option<Bounds<Pixels>> {
        cx.try_global::<Self>()?.cell(slot).get()
    }

    pub fn covers(cx: &App) -> Covers {
        cx.try_global::<Self>()
            .map(|s| s.covers.borrow().clone())
            .unwrap_or_default()
    }
}

/// An empty box filling its parent (which must be `relative`) that records
/// its bounds as `which`.
pub fn slot(which: Slot) -> impl IntoElement {
    canvas(
        move |bounds, _, cx| {
            if let Some(slots) = cx.try_global::<Slots>() {
                slots.cell(which).set(Some(bounds));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

/// Tells the effects which cover images the app shows.
pub fn set_covers(covers: Covers, cx: &App) {
    if let Some(slots) = cx.try_global::<Slots>() {
        slots.covers.replace(covers);
    }
}

/// The page panel: Now Playing's area, extended up to the panel's top edge
/// (the panel sits `space::SM` below the window's top).
pub fn panel(cx: &App) -> Option<Bounds<Pixels>> {
    let page = Slots::get(cx, Slot::Page)?;
    let top = crate::theme::space::SM;
    Some(Bounds::from_corners(
        point(page.left(), top),
        page.bottom_right(),
    ))
}
