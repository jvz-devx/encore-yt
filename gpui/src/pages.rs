//! M1: where the app is (view, history), the pages it asked for, their
//! continuations, and search (`search`).

mod list;
pub mod search;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Chip, Item, Page, Shelf, Target, Track};
use ytfast::parse::More;

use crate::app::MusicApp;
use crate::nav::{LibraryTab, PageState, View};

pub use list::PageList;
pub use search::{Dropdown, Search};

actions!(music, [Back, Forward]);

/// A page older than this is fetched again when it's opened.
const STALE: Duration = Duration::from_secs(300);
/// How many views Back remembers.
const HISTORY: usize = 50;

pub struct Pages {
    pub view: View,
    pub history: Vec<View>,
    pub forward: Vec<View>,
    pub states: HashMap<String, PageState>,
    seq: u64,
    pub search: Search,
    /// Each page's virtual list (and so its scroll position), by page key.
    pub lists: HashMap<String, PageList>,
    /// Horizontal scroll of carousels, by page key and shelf.
    pub carousels: HashMap<(String, usize), ScrollHandle>,
    /// Pages whose header description is shown in full.
    pub expanded: HashSet<String>,
    /// The subtitle link under the pointer: (text element id, run index).
    pub link_hover: Option<(SharedString, usize)>,
}

impl Pages {
    pub fn new(_window: &mut Window, _cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        (
            Self {
                view: View::Home,
                history: Vec::new(),
                forward: Vec::new(),
                states: HashMap::new(),
                seq: 0,
                search: Search::default(),
                lists: HashMap::new(),
                carousels: HashMap::new(),
                expanded: HashSet::new(),
                link_hover: None,
            },
            Vec::new(),
        )
    }

    /// A carousel's scroll handle, made on first use.
    pub fn carousel(&mut self, key: &str, shelf: usize) -> ScrollHandle {
        self.carousels
            .entry((key.to_string(), shelf))
            .or_default()
            .clone()
    }
}

impl MusicApp {
    pub fn open(&mut self, view: View, cx: &mut Context<Self>) {
        let pages = &mut self.pages;
        if view != pages.view {
            let previous = std::mem::replace(&mut pages.view, view);
            pages.history.push(previous);
            pages.forward.clear();
            if pages.history.len() > HISTORY {
                pages.history.remove(0);
            }
        }
        // A page opened anew starts at its top; Back and Forward keep the
        // place it was left at.
        if let Some(list) = self.pages.lists.get(&self.pages.view.target().key()) {
            list.scroll_to_top();
        }
        self.player.now_playing = false;
        self.ensure_page(self.pages.view.target(), false);
        cx.notify();
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.pages.history.pop() {
            let left = std::mem::replace(&mut self.pages.view, view);
            self.pages.forward.push(left);
            self.player.now_playing = false;
            self.ensure_page(self.pages.view.target(), false);
            cx.notify();
        }
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.pages.forward.pop() {
            let left = std::mem::replace(&mut self.pages.view, view);
            self.pages.history.push(left);
            self.player.now_playing = false;
            self.ensure_page(self.pages.view.target(), false);
            cx.notify();
        }
    }

    /// Open a target's page, or start playing it.
    pub fn activate(&mut self, target: Target, cx: &mut Context<Self>) {
        match View::for_target(&target) {
            Some(view) => self.open(view, cx),
            None => self.send(Command::PlayTarget(target)),
        }
    }

    /// What clicking an item does: play a song with its shelf as the queue,
    /// or open (or play) its target.
    pub fn activate_item(&mut self, item: &Item, shelf: &Shelf, cx: &mut Context<Self>) {
        if let Some(track) = &item.track {
            let tracks: Vec<Track> = shelf.items.iter().filter_map(|i| i.track.clone()).collect();
            let start = tracks
                .iter()
                .position(|t| t.video_id == track.video_id)
                .unwrap_or(0);
            self.send(Command::PlayTracks { tracks, start });
        } else if let Some(target) = &item.target {
            self.activate(target.clone(), cx);
        } else if let Some(play) = &item.play {
            self.send(Command::PlayTarget(play.clone()));
        }
    }

    /// The cover's play button: an album or playlist plays (its own play
    /// target) instead of opening.
    pub fn play_item(&mut self, item: &Item, shelf: &Shelf, cx: &mut Context<Self>) {
        match (&item.play, &item.track) {
            (Some(play), None) => self.send(Command::PlayTarget(play.clone())),
            _ => self.activate_item(item, shelf, cx),
        }
    }

    /// The header's Play: its play target, or else the page's songs in order.
    pub fn play_page(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(page) = self.pages.states.get(key).and_then(|s| s.page.as_ref()) else {
            return;
        };
        if let Some(play) = page.header.as_ref().and_then(|h| h.play.clone()) {
            self.activate(play, cx);
            return;
        }
        let tracks: Vec<Track> = page
            .shelves
            .iter()
            .flat_map(|s| s.items.iter())
            .filter_map(|i| i.track.clone())
            .collect();
        if !tracks.is_empty() {
            self.send(Command::PlayTracks { tracks, start: 0 });
            self.sidebar_played_page(key);
        }
    }

    /// Asks for a page unless a fresh copy is loaded or loading.
    pub fn ensure_page(&mut self, target: Target, force: bool) {
        let key = target.key();
        let needed = match self.pages.states.get(&key) {
            None => true,
            // A forced refresh wins over a fetch already under way (sign-in).
            Some(s) => {
                force
                    || (!s.loading
                        && (s.error.is_some() || s.fetched.is_none_or(|t| t.elapsed() > STALE)))
            }
        };
        if !needed {
            return;
        }
        self.pages.seq += 1;
        let seq = self.pages.seq;
        let state = self.pages.states.entry(key).or_insert_with(|| PageState {
            target: target.clone(),
            page: None,
            loading: false,
            cached: false,
            error: None,
            more_loading: HashSet::new(),
            fetched: None,
            seq,
            reload: None,
        });
        state.loading = true;
        state.error = None;
        state.reload = None;
        state.seq = seq;
        self.backend.send(Command::Page { target, seq });
    }

    /// Asks again for a page that failed or shows its saved copy.
    pub fn retry_page(&mut self, key: &str, cx: &mut Context<Self>) {
        if let Some(target) = self.pages.states.get(key).map(|s| s.target.clone()) {
            self.ensure_page(target, true);
            cx.notify();
        }
    }

    pub(crate) fn on_page(
        &mut self,
        key: String,
        seq: u64,
        result: Result<Box<Page>, String>,
        cached: bool,
    ) {
        let Some(state) = self.pages.states.get_mut(&key) else {
            return;
        };
        if seq != state.seq {
            return;
        }
        match result {
            Ok(page) => {
                // A late cached copy never replaces fresh content.
                if cached && state.page.is_some() && !state.cached {
                    return;
                }
                state.page = Some(*page);
                state.cached = cached;
                if !cached {
                    state.loading = false;
                    state.error = None;
                    state.fetched = Some(Instant::now());
                    state.more_loading.clear();
                }
            }
            Err(error) => {
                state.loading = false;
                state.error = Some(error);
            }
        }
    }

    /// Loads the next part of page `key`: its own continuation (more
    /// shelves, Home), or else the last shelf that has one (long playlists,
    /// library lists). Called when the end of the page comes into view.
    pub fn more_page(&mut self, key: &str) {
        let Some(state) = self.pages.states.get(key) else {
            return;
        };
        let Some(page) = &state.page else { return };
        if state.cached {
            return;
        }
        if page.continuation.is_some() {
            self.more(key, None);
            return;
        }
        if let Some(i) = page.shelves.iter().rposition(|s| s.continuation.is_some()) {
            self.more(key, Some(i));
        }
    }

    /// Loads the next part of page `key` (`None`) or of its shelf `shelf`,
    /// unless that is already on its way.
    pub fn more(&mut self, key: &str, shelf: Option<usize>) {
        let Some(state) = self.pages.states.get_mut(key) else {
            return;
        };
        let Some(page) = &state.page else { return };
        if state.cached {
            return;
        }
        let token = match shelf {
            None => page.continuation.clone(),
            Some(i) => page.shelves.get(i).and_then(|s| s.continuation.clone()),
        };
        let Some(token) = token else { return };
        if !state.more_loading.insert(shelf) {
            return;
        }
        let search = matches!(state.target, Target::Search { .. });
        log::info!(
            "loading more of {key} ({})",
            shelf.map_or("page".to_string(), |i| format!("shelf {i}"))
        );
        self.backend.send(Command::More {
            key: key.to_string(),
            token,
            search,
            shelf,
        });
    }

    pub(crate) fn on_more(
        &mut self,
        key: String,
        shelf: Option<usize>,
        token: String,
        result: Result<More, String>,
    ) {
        let Some(state) = self.pages.states.get_mut(&key) else {
            return;
        };
        let Some(page) = state.page.as_mut() else {
            return;
        };
        // A chip's in-place reload (an artist's Albums / Singles & EPs).
        if shelf.is_none() && state.reload.as_ref().is_some_and(|(t, _)| *t == token) {
            let chip = state.reload.take().map_or(0, |(_, chip)| chip);
            state.more_loading.remove(&None);
            match result {
                Ok(More::Shelves { shelves, next }) => {
                    page.shelves = shelves;
                    page.continuation = next;
                    for (i, c) in page.chips.iter_mut().enumerate() {
                        c.selected = i == chip;
                    }
                }
                Ok(More::Items { .. }) => {}
                Err(error) => self.error = Some(format!("Couldn't load that: {error}")),
            }
            return;
        }
        // Only the answer to the token still on the page applies; a
        // refreshed page has its own.
        let slot = match shelf {
            None => &mut page.continuation,
            Some(i) => match page.shelves.get_mut(i) {
                Some(s) => &mut s.continuation,
                None => return,
            },
        };
        if slot.as_deref() != Some(token.as_str()) {
            return;
        }
        state.more_loading.remove(&shelf);
        match result {
            Ok(More::Shelves { shelves, next }) => {
                log::info!("{key}: {} more shelves", shelves.len());
                page.shelves.extend(shelves);
                page.continuation = next;
            }
            Ok(More::Items { items, next }) => {
                log::info!("{key}: {} more items", items.len());
                match shelf.and_then(|i| page.shelves.get_mut(i)) {
                    Some(s) => {
                        s.items.extend(items);
                        s.continuation = next;
                    }
                    None => page.continuation = None,
                }
            }
            Err(error) => {
                // Stop asking for this part; a page refresh starts over.
                *slot = None;
                self.error = Some(format!("Couldn't load more: {error}"));
            }
        }
    }

    /// Chip `i` of page `key` chosen. A selected one goes back where it came
    /// from (a Home mood back to Home); another opens its page or swaps the
    /// shelves in place.
    pub fn choose_chip(&mut self, key: &str, i: usize, cx: &mut Context<Self>) {
        let Some(chip) = self
            .pages
            .states
            .get(key)
            .and_then(|s| s.page.as_ref())
            .and_then(|p| p.chips.get(i))
            .cloned()
        else {
            return;
        };
        let Chip {
            selected,
            deselect,
            reload,
            target,
            ..
        } = chip;
        if selected {
            match deselect {
                Some(Target::Browse { id, .. }) if id == "FEmusic_home" => {
                    self.open(View::Home, cx)
                }
                Some(target) => self.activate(target, cx),
                None => {}
            }
        } else if let Some(token) = reload {
            self.reload_chip(key, i, token);
            cx.notify();
        } else if let Some(target) = target {
            self.activate(target, cx);
        }
    }

    fn reload_chip(&mut self, key: &str, chip: usize, token: String) {
        let Some(state) = self.pages.states.get_mut(key) else {
            return;
        };
        if state.page.is_none() {
            return;
        }
        state.reload = Some((token.clone(), chip));
        state.more_loading.insert(None);
        self.backend.send(Command::More {
            key: key.to_string(),
            token,
            search: false,
            shelf: None,
        });
    }

    /// The Library tab chips.
    pub fn open_library(&mut self, tab: LibraryTab, cx: &mut Context<Self>) {
        if self.pages.view != View::Library(tab) {
            self.open(View::Library(tab), cx);
        }
    }

    /// Shows or hides the whole of page `key`'s description.
    pub fn toggle_description(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.pages.expanded.remove(key) {
            self.pages.expanded.insert(key.to_string());
        }
        cx.notify();
    }

    /// YouTube Music's answer for Play anything (Ctrl+K, desktop).
    pub(crate) fn on_quick_results(&mut self, query: String, result: Result<Box<Page>, String>) {
        self.quick_results(query, result);
    }
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-left", Back, Some("Music")),
        KeyBinding::new("alt-right", Forward, Some("Music")),
    ]);
}

/// Handlers for this area's actions, on the window's root element: Back and
/// Forward from the keyboard and the mouse's side buttons.
pub fn on_actions(root: Div, cx: &mut Context<MusicApp>) -> Div {
    root.on_action(cx.listener(|this, _: &Back, _, cx| this.back(cx)))
        .on_action(cx.listener(|this, _: &Forward, _, cx| this.go_forward(cx)))
        .on_mouse_down(
            MouseButton::Navigate(NavigationDirection::Back),
            cx.listener(|this, _, _, cx| this.back(cx)),
        )
        .on_mouse_down(
            MouseButton::Navigate(NavigationDirection::Forward),
            cx.listener(|this, _, _, cx| this.go_forward(cx)),
        )
}
