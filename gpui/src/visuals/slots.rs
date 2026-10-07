//! Where the views want their effects. Now Playing and the player bar
//! (inside the cached app view) lay out empty boxes with [`slot`]; their
//! bounds land here during the app's prepaint, and the effects and flight
//! layers read them when they paint, in the same frame or, while the app
//! view is reused, from before.

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
    /// Now Playing's waveform.
    Waveform,
    /// The player bar.
    Bar,
    /// The player bar's seek slider (its 24 point box).
    Seek,
    /// The play button.
    Play,
    /// The player bar's cover.
    BarCover,
    /// The whole window under Stage or the full-window visualiser.
    Stage,
    /// The scene's area for the visualiser (above Stage's transport).
    StageBody,
    /// The scene's cover.
    StageCover,
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
    waveform: Cell<Option<Bounds<Pixels>>>,
    bar: Cell<Option<Bounds<Pixels>>>,
    seek: Cell<Option<Bounds<Pixels>>>,
    play: Cell<Option<Bounds<Pixels>>>,
    bar_cover: Cell<Option<Bounds<Pixels>>>,
    stage: Cell<Option<Bounds<Pixels>>>,
    stage_body: Cell<Option<Bounds<Pixels>>>,
    stage_cover: Cell<Option<Bounds<Pixels>>>,
    covers: RefCell<Covers>,
    /// The effects layer paints the player bar's background and seek bar.
    bar_painted: Cell<bool>,
    /// The backdrop draws the large cover's drop shadow.
    shadow_painted: Cell<bool>,
}

impl Global for Slots {}

impl Slots {
    fn cell(&self, slot: Slot) -> &Cell<Option<Bounds<Pixels>>> {
        match slot {
            Slot::Page => &self.page,
            Slot::Cover => &self.cover,
            Slot::Spectrum => &self.spectrum,
            Slot::Waveform => &self.waveform,
            Slot::Bar => &self.bar,
            Slot::Seek => &self.seek,
            Slot::Play => &self.play,
            Slot::BarCover => &self.bar_cover,
            Slot::Stage => &self.stage,
            Slot::StageBody => &self.stage_body,
            Slot::StageCover => &self.stage_cover,
        }
    }

    pub fn get(cx: &App, slot: Slot) -> Option<Bounds<Pixels>> {
        cx.try_global::<Self>()?.cell(slot).get()
    }

    pub fn set_bar_painted(cx: &App, painted: bool) {
        if let Some(slots) = cx.try_global::<Self>() {
            slots.bar_painted.set(painted);
        }
    }

    pub fn set_shadow_painted(cx: &App, painted: bool) {
        if let Some(slots) = cx.try_global::<Self>() {
            slots.shadow_painted.set(painted);
        }
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

/// Whether the effects layer paints the player bar (its glow and seek bar):
/// the bar then leaves its background and slider see-through.
pub fn paints_bar(cx: &App) -> bool {
    cx.try_global::<Slots>()
        .is_some_and(|s| s.bar_painted.get())
}

/// Whether the backdrop draws Now Playing's cover shadow: the cover then
/// leaves its own out.
pub fn paints_cover_shadow(cx: &App) -> bool {
    cx.try_global::<Slots>()
        .is_some_and(|s| s.shadow_painted.get())
}

/// Tells the effects which cover image the player bar shows.
pub fn set_bar_cover(url: Option<SharedString>, cx: &App) {
    if let Some(slots) = cx.try_global::<Slots>() {
        slots.covers.borrow_mut().small = url;
    }
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
