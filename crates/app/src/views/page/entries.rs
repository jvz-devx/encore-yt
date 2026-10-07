//! A page as the entries of its virtual list: the header, the chips, each
//! shelf, and each row of a song list on its own, so a playlist of hundreds
//! of songs lays out only the rows on screen.

use std::hash::{DefaultHasher, Hash, Hasher};

use encore_core::model::{Page, ShelfStyle, Target};
use gpui_kit::Pixels;

use crate::nav::{LibraryTab, PageState};
use crate::theme::space;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Entry {
    /// The Library's tab chips.
    Tabs(LibraryTab),
    /// The page shown is the saved copy and the refresh failed.
    Saved,
    /// "Results for …" above search results.
    Caption,
    Header,
    Chips,
    /// A chip is swapping the shelves in place.
    Reloading,
    /// No shelves: YouTube's message, or the Library signed out.
    Empty,
    /// A shelf drawn whole (cards, Quick picks, buttons, top result).
    Shelf(usize),
    /// A song list's title; its rows follow.
    ListTitle(usize),
    Row(usize, usize),
    /// Under a song list with more rows to load: loads them when it comes
    /// into view, and shows their shape while they load.
    ListMore(usize),
    /// More shelves on their way.
    PageMore,
    /// The end of the page: loads the next part when it comes into view.
    End,
}

/// The entries of `state`'s page, top to bottom.
pub fn build(state: &PageState, tab: Option<LibraryTab>) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let Some(tab) = tab {
        entries.push(Entry::Tabs(tab));
    }
    let Some(page) = &state.page else {
        return entries;
    };
    if state.error.is_some() {
        entries.push(Entry::Saved);
    }
    if matches!(state.target, Target::Search { .. }) {
        entries.push(Entry::Caption);
    }
    if page.header.is_some() {
        entries.push(Entry::Header);
    }
    // The Library's own single chip repeats its tab.
    let chips = page.chips.len() > usize::from(tab.is_some());
    if chips {
        entries.push(Entry::Chips);
    }
    if state.reload.is_some() {
        entries.push(Entry::Reloading);
        return entries;
    }
    if page.shelves.is_empty() {
        entries.push(Entry::Empty);
        return entries;
    }
    for (i, shelf) in page.shelves.iter().enumerate() {
        if shelf.style != ShelfStyle::List {
            entries.push(Entry::Shelf(i));
            continue;
        }
        if !shelf.title.is_empty() || shelf.more.is_some() {
            entries.push(Entry::ListTitle(i));
        }
        entries.extend((0..shelf.items.len()).map(|j| Entry::Row(i, j)));
        if shelf.continuation.is_some() {
            entries.push(Entry::ListMore(i));
        }
    }
    if state.more_loading.contains(&None) {
        entries.push(Entry::PageMore);
    }
    entries.push(Entry::End);
    entries
}

/// A signature of what an entry draws that changes its height: entries
/// whose signature changed are measured again.
pub fn signature(entry: &Entry, state: &PageState, expanded: bool) -> u64 {
    let mut h = DefaultHasher::new();
    entry.hash(&mut h);
    let Some(page) = &state.page else {
        return h.finish();
    };
    match *entry {
        Entry::Saved => state.error.hash(&mut h),
        Entry::Header => {
            if let Some(header) = &page.header {
                header.title.hash(&mut h);
                header.description.hash(&mut h);
                header.second_subtitle.hash(&mut h);
                expanded.hash(&mut h);
            }
        }
        Entry::Chips => {
            for chip in &page.chips {
                chip.text.hash(&mut h);
                chip.selected.hash(&mut h);
            }
        }
        Entry::Empty => page.message.hash(&mut h),
        Entry::Shelf(i) | Entry::ListTitle(i) => shelf_signature(page, i, &mut h),
        Entry::ListMore(i) => state.more_loading.contains(&Some(i)).hash(&mut h),
        _ => {}
    }
    h.finish()
}

fn shelf_signature(page: &Page, i: usize, h: &mut DefaultHasher) {
    let Some(shelf) = page.shelves.get(i) else {
        return;
    };
    shelf.title.hash(h);
    shelf.strapline.hash(h);
    shelf.more.is_some().hash(h);
    shelf.items.len().hash(h);
    if let Some(first) = shelf.items.first() {
        first.title.hash(h);
    }
}

/// The space above an entry, given the one before it.
pub fn space_above(entry: &Entry, previous: Option<&Entry>) -> Pixels {
    let Some(previous) = previous else {
        return space::SM;
    };
    match (previous, entry) {
        (Entry::Row(..) | Entry::ListTitle(_), Entry::Row(..) | Entry::ListMore(_)) => Pixels::ZERO,
        (_, Entry::End) => Pixels::ZERO,
        (Entry::Tabs(_) | Entry::Saved | Entry::Caption, _) => space::XL,
        (Entry::Chips, _) => space::XL,
        (Entry::Header, _) => space::XL + space::SM,
        _ => space::XXL + space::SM,
    }
}
