//! A page: its header, chips and shelves in one virtual list, so long song
//! lists lay out only what is on screen and each page keeps its place for
//! Back and Forward.
//!
//! Entries are drawn while the list lays itself out, inside an update of
//! the app; clicks look their item up again by (page key, shelf, item) when
//! they happen, so a frame never clones the shelves into its closures.

mod card;
mod chips;
pub mod covers;
mod entries;
mod header;
mod row;
mod runs;
mod shelf;
mod states;
mod transition;

use std::rc::Rc;

use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::v_flex;
use gpui_kit::*;
use ytfast::model::{Item, Page, Shelf};

use crate::app::MusicApp;
use crate::nav::View;
use crate::theme::motion::{self, MotionExt as _};
use crate::theme::{self, Colors, size, space};
use entries::Entry;

/// What drawing an entry needs besides the page.
pub(super) struct Ctx {
    pub key: String,
    /// The video id playing now.
    pub playing: Option<String>,
    /// The subtitle link under the pointer.
    pub link: Option<(SharedString, usize)>,
    /// Which songs are liked (M25).
    pub likes: crate::likes::Likes,
    pub c: Colors,
}

impl Ctx {
    fn new(app: &MusicApp, key: &str, cx: &App) -> Self {
        Self {
            key: key.to_string(),
            playing: app.player.current().map(|t| t.video_id.clone()),
            link: app.pages.link_hover.clone(),
            likes: crate::likes::Likes::of(app),
            c: theme::colors(cx),
        }
    }

    /// An element id unique to this page.
    pub fn id(&self, what: impl std::fmt::Display) -> SharedString {
        SharedString::from(format!("{}:{what}", self.key))
    }
}

/// The current view's page, arriving with the page transition after a
/// move.
pub fn page(app: &mut MusicApp, window: &mut Window, cx: &mut Context<MusicApp>) -> AnyElement {
    let content = content(app, cx);
    transition::arrive(app, content, window)
}

fn content(app: &mut MusicApp, cx: &mut Context<MusicApp>) -> AnyElement {
    let target = app.pages.view.target();
    let key = target.key();
    let tab = match app.pages.view {
        View::Library(tab) => Some(tab),
        _ => None,
    };
    let Some(state) = app.pages.states.get(&key) else {
        return states::loading(app, &target, tab, cx).into_any_element();
    };
    if state.page.is_none() {
        return match state.error.clone() {
            Some(error) => states::failed(app, error, &key, tab, cx).into_any_element(),
            None => states::loading(app, &target, tab, cx).into_any_element(),
        };
    }
    let entries = entries::build(state, tab);
    let expanded = app.pages.expanded.contains(&key);
    let sigs = entries
        .iter()
        .map(|e| entries::signature(e, state, expanded))
        .collect();
    let list_state = {
        let list = app.pages.lists.entry(key.clone()).or_default();
        list.sync(sigs);
        list.state.clone()
    };
    let entries = Rc::new(entries);
    let this = cx.entity().downgrade();
    let list_key = key.clone();
    let body = list(list_state.clone(), move |ix, window, cx| {
        let Some(entry) = entries.get(ix).copied() else {
            return div().into_any_element();
        };
        let previous = ix.checked_sub(1).and_then(|i| entries.get(i));
        let top = entries::space_above(&entry, previous);
        let cache = covers::nested_cache(cx);
        let drawn = this
            .update(cx, |app, cx| draw_entry(app, &list_key, entry, window, cx))
            .unwrap_or_else(|_| div().into_any_element());
        image_cache(cache)
            .w_full()
            .px(size::GUTTER)
            .pt(top)
            .child(drawn)
            .into_any_element()
    })
    .size_full();
    let page = v_flex()
        .id(SharedString::from(format!("page:{key}")))
        .flex_1()
        .w_full()
        .min_h_0()
        .relative()
        .child(body)
        .vertical_scrollbar(&list_state);
    if app.pages.transition.ready {
        return page.into_any_element();
    }
    // A page that arrives after its skeleton settles in rather than
    // popping.
    page.with_motion(
        SharedString::from(format!("enter:{key}")),
        motion::Kind::Pages,
        motion::BASE,
        |el, t| el.opacity(t),
    )
}

/// Draws one entry of page `key`.
fn draw_entry(
    app: &mut MusicApp,
    key: &str,
    entry: Entry,
    _window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let ctx = Ctx::new(app, key, cx);
    let carousel = match entry {
        Entry::Shelf(i) => Some(app.pages.carousel(key, i)),
        _ => None,
    };
    let Some(state) = app.pages.states.get(key) else {
        return div().into_any_element();
    };
    let Some(page) = &state.page else {
        return div().into_any_element();
    };
    match entry {
        Entry::Tabs(tab) => {
            let tabs = chips::library_tabs(tab, &ctx, cx);
            // Library → Playlists: New playlist beside the tabs (M3).
            let actions = (tab == crate::nav::LibraryTab::Playlists)
                .then(|| super::account::library_actions(app, cx))
                .flatten();
            match actions {
                Some(actions) => gpui_kit::component::h_flex()
                    .w_full()
                    .gap(space::SM)
                    .child(div().flex_1().min_w_0().child(tabs))
                    .child(actions)
                    .into_any_element(),
                None => tabs,
            }
        }
        Entry::Saved => states::saved(state.error.clone().unwrap_or_default(), &ctx, cx),
        Entry::Caption => states::caption(&state.target, &ctx),
        Entry::Header => match &page.header {
            Some(h) => header::header(app, h, &ctx, cx),
            None => div().into_any_element(),
        },
        Entry::Chips => chips::page_chips(&page.chips, &ctx, cx),
        Entry::Reloading => states::skeleton_shelf(&ctx.c).into_any_element(),
        Entry::Empty => states::empty(app, page, &ctx, cx),
        Entry::Shelf(i) => match (page.shelves.get(i), carousel) {
            (Some(s), Some(scroll)) => shelf::shelf(i, s, scroll, &ctx, cx),
            _ => div().into_any_element(),
        },
        Entry::ListTitle(i) => match page.shelves.get(i) {
            Some(s) => shelf::title(i, s, &ctx, cx),
            None => div().into_any_element(),
        },
        Entry::Row(i, j) => match page.shelves.get(i).and_then(|s| Some((s, s.items.get(j)?))) {
            Some((s, item)) => row::row(i, j, item, row::Album::of(s), &ctx, cx),
            None => div().into_any_element(),
        },
        Entry::ListMore(i) => {
            // In view: ask for the next rows.
            app.more(key, Some(i));
            states::skeleton_rows(3, &ctx.c).into_any_element()
        }
        Entry::PageMore => states::skeleton_shelf(&ctx.c).into_any_element(),
        Entry::End => {
            app.more_page(key);
            div().h(space::XXXL).into_any_element()
        }
    }
}

/// Finds item `item` of shelf `shelf` on page `key` when a click happens.
fn find(app: &MusicApp, key: &str, shelf: usize, item: usize) -> Option<(Item, Shelf)> {
    let page: &Page = app.pages.states.get(key)?.page.as_ref()?;
    let shelf = page.shelves.get(shelf)?;
    Some((shelf.items.get(item)?.clone(), shelf.clone()))
}

/// The click handler for item `item` of shelf `shelf`: play a song (its
/// shelf as the queue), or open its page.
pub(super) fn on_activate(
    ctx: &Ctx,
    shelf: usize,
    item: usize,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let key = ctx.key.clone();
    cx.listener(move |this, _: &ClickEvent, _, cx| {
        if let Some((item, shelf)) = find(this, &key, shelf, item) {
            this.activate_item(&item, &shelf, cx);
        }
    })
}

/// The cover's play button: plays the item's own play target where it has
/// one (an album, a playlist) instead of opening it.
pub(super) fn on_play(
    ctx: &Ctx,
    shelf: usize,
    item: usize,
    cx: &mut Context<MusicApp>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let key = ctx.key.clone();
    cx.listener(move |this, _: &ClickEvent, _, cx| {
        cx.stop_propagation();
        if let Some((item, shelf)) = find(this, &key, shelf, item) {
            this.play_item(&item, &shelf, cx);
        }
    })
}
