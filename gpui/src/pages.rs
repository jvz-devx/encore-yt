//! M1: where the app is (view, history), the pages it asked for, and search.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Item, Page, Shelf, Target, Track};
use ytfast::parse::More;

use crate::app::MusicApp;
use crate::nav::{PageState, View};

actions!(music, [Back, Forward]);

/// A page older than this is fetched again when it's opened.
const STALE: Duration = Duration::from_secs(300);

pub struct Pages {
    pub view: View,
    pub history: Vec<View>,
    pub forward: Vec<View>,
    pub states: HashMap<String, PageState>,
    seq: u64,
    pub search: Entity<InputState>,
}

impl Pages {
    pub fn new(window: &mut Window, cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search songs, albums, artists, podcasts")
        });
        let subscriptions =
            vec![
                cx.subscribe_in(&search, window, |this, state, event: &InputEvent, _, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        let query = state.read(cx).value().trim().to_string();
                        if !query.is_empty() {
                            this.open(
                                View::Page(Target::Search {
                                    query,
                                    params: None,
                                }),
                                cx,
                            );
                        }
                    }
                }),
            ];
        (
            Self {
                view: View::Home,
                history: Vec::new(),
                forward: Vec::new(),
                states: HashMap::new(),
                seq: 0,
                search,
            },
            subscriptions,
        )
    }
}

impl MusicApp {
    pub fn open(&mut self, view: View, cx: &mut Context<Self>) {
        let pages = &mut self.pages;
        if view != pages.view {
            let previous = std::mem::replace(&mut pages.view, view);
            pages.history.push(previous);
            pages.forward.clear();
            if pages.history.len() > 50 {
                pages.history.remove(0);
            }
        }
        self.ensure_page(self.pages.view.target(), false);
        cx.notify();
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.pages.history.pop() {
            let left = std::mem::replace(&mut self.pages.view, view);
            self.pages.forward.push(left);
            self.ensure_page(self.pages.view.target(), false);
            cx.notify();
        }
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.pages.forward.pop() {
            let left = std::mem::replace(&mut self.pages.view, view);
            self.pages.history.push(left);
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

    pub fn ensure_page(&mut self, target: Target, force: bool) {
        let key = target.key();
        let needed = match self.pages.states.get(&key) {
            None => true,
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
            page: None,
            loading: false,
            cached: false,
            error: None,
            fetched: None,
            seq,
        });
        state.loading = true;
        state.error = None;
        state.seq = seq;
        self.backend.send(Command::Page { target, seq });
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
                }
            }
            Err(error) => {
                state.loading = false;
                state.error = Some(error);
            }
        }
    }

    pub(crate) fn on_more(
        &mut self,
        _key: String,
        _shelf: Option<usize>,
        _token: String,
        _result: Result<More, String>,
    ) {
        // M1: continuations.
    }

    pub(crate) fn on_suggestions(&mut self, _input: String, _items: Vec<String>) {
        // M1: search suggestions.
    }

    pub(crate) fn on_searches(&mut self, _saved: Vec<String>) {
        // M1: recent searches.
    }

    pub(crate) fn on_quick_results(&mut self, _query: String, _result: Result<Box<Page>, String>) {
        // M4: Play anything (Ctrl+K).
    }
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("alt-left", Back, Some("Music")),
        KeyBinding::new("alt-right", Forward, Some("Music")),
    ]);
}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, cx: &mut Context<MusicApp>) -> Div {
    root.on_action(cx.listener(|this, _: &Back, _, cx| this.back(cx)))
        .on_action(cx.listener(|this, _: &Forward, _, cx| this.go_forward(cx)))
}
