//! Play anything (Ctrl+K): one field over the library pages already
//! loaded, recent searches, the page on screen, YouTube Music's search and
//! a few commands. Local results come with each keystroke; YouTube Music is
//! asked once typing pauses and its answer joins in. Ranking: exact and
//! prefix title matches first, then the library over the catalogue.
//! Enter plays the highlighted result, Shift+Enter opens its page.

mod search;

pub use search::{Cmd, Go, Hit, Kind};
use search::{catalogue_query, library_targets, rank};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::Duration;

use encore_core::backend::Command;
use encore_core::model::{Page, Track};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::control::{self, FromPage};
use crate::app::MusicApp;

/// Typing pauses this long before YouTube Music is asked.
const DEBOUNCE: Duration = Duration::from_millis(150);
static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub struct PlayAnything {
    pub input: Entity<InputState>,
    pub query: String,
    pub selected: usize,
    /// The catalogue search last asked for.
    asked: String,
    /// YouTube Music's last answer: its query and results.
    catalogue: Option<(String, Result<Page, String>)>,
    generation: u64,
    hits: Vec<Hit>,
    /// What `hits` were worked out from.
    stamp: u64,
    /// A note under the results ("not available yet").
    pub note: Option<&'static str>,
    pub serial: u64,
    debounce: Option<Task<()>>,
    _subscription: Subscription,
}

impl PlayAnything {
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// YouTube Music hasn't answered for what's typed yet.
    pub fn searching(&self) -> bool {
        catalogue_query(&self.query).is_some_and(|q| {
            self.catalogue
                .as_ref()
                .is_none_or(|(answered, _)| *answered != q)
        })
    }

    /// YouTube Music couldn't be asked.
    pub fn failed(&self) -> bool {
        matches!(&self.catalogue, Some((_, Err(_))))
    }
}

impl MusicApp {
    /// Ctrl+K: opens Play anything empty, or closes it.
    pub fn toggle_play_anything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.desktop.layers.palette.is_some() {
            self.close_play_anything(window, cx);
            return;
        }
        self.close_layers(window, cx);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Play anything: songs, albums, artists, commands")
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_play_anything_input);
        input.update(cx, |state, cx| state.focus(window, cx));
        log::info!("Play anything opened");
        self.desktop.layers.palette = Some(PlayAnything {
            input,
            query: String::new(),
            selected: 0,
            asked: String::new(),
            catalogue: None,
            generation: 0,
            hits: Vec::new(),
            stamp: 0,
            note: None,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            debounce: None,
            _subscription: subscription,
        });
        self.refresh_hits();
        cx.notify();
    }

    pub fn close_play_anything(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.desktop.layers.palette.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn on_play_anything_input(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = state.read(cx).value().to_string();
                if let Some(pa) = &mut self.desktop.layers.palette {
                    pa.query = query;
                    pa.selected = 0;
                    pa.note = None;
                }
                self.refresh_hits();
                self.ask_catalogue_later(cx);
            }
            InputEvent::PressEnter { shift, .. } => {
                let selected = self.desktop.layers.palette.as_ref().map(|p| p.selected);
                if let Some(i) = selected {
                    self.choose_hit(i, *shift, window, cx);
                }
            }
            InputEvent::Focus | InputEvent::Blur => {}
        }
        cx.notify();
    }

    /// The arrows: moves the highlight, wrapping.
    pub fn move_in_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(pa) = &mut self.desktop.layers.palette else {
            return;
        };
        let n = pa.hits.len() as isize;
        if n == 0 {
            return;
        }
        pa.selected = (pa.selected as isize + delta).rem_euclid(n) as usize;
        let song = match pa.hits.get(pa.selected).map(|h| &h.go) {
            Some(Go::Song(track)) => Some(track.video_id.clone()),
            _ => None,
        };
        if let Some(id) = song {
            // Ready before Enter.
            self.send(Command::Prepare(id));
        }
        cx.notify();
    }

    /// Asks YouTube Music once typing pauses.
    fn ask_catalogue_later(&mut self, cx: &mut Context<Self>) {
        let Some(pa) = &mut self.desktop.layers.palette else {
            return;
        };
        let Some(query) = catalogue_query(&pa.query) else {
            pa.debounce = None;
            return;
        };
        if query == pa.asked {
            return;
        }
        pa.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                let Some(pa) = &mut this.desktop.layers.palette else {
                    return;
                };
                if catalogue_query(&pa.query).as_deref() == Some(query.as_str()) {
                    pa.asked = query.clone();
                    log::info!("Play anything: asking YouTube Music for {query:?}");
                    this.send(Command::QuickSearch(query));
                    cx.notify();
                }
            });
        }));
    }

    /// YouTube Music's answer; only the one for the latest search is kept.
    pub(crate) fn quick_results(&mut self, query: String, result: Result<Box<Page>, String>) {
        let Some(pa) = &mut self.desktop.layers.palette else {
            return;
        };
        if query != pa.asked {
            return;
        }
        match &result {
            Ok(page) => log::info!(
                "Play anything: {} shelves from YouTube Music for {query:?}",
                page.shelves.len()
            ),
            Err(e) => log::warn!("Play anything: YouTube Music failed: {e}"),
        }
        pa.catalogue = Some((query, result.map(|page| *page)));
        pa.generation += 1;
        self.refresh_hits();
    }

    /// Works the results out again when what they come from changed.
    pub fn refresh_hits(&mut self) {
        let Some(pa) = &self.desktop.layers.palette else {
            return;
        };
        let stamp = self.hits_stamp(pa);
        if stamp == pa.stamp {
            return;
        }
        let hits = rank(
            &pa.query,
            &self.pages.search.recent,
            &self.pages.view.target(),
            |target| self.pages.states.get(&target.key())?.page.as_ref(),
            pa.catalogue.as_ref(),
        );
        if let Some(pa) = &mut self.desktop.layers.palette {
            pa.hits = hits;
            pa.stamp = stamp;
            pa.selected = pa.selected.min(pa.hits.len().saturating_sub(1));
        }
    }

    /// Changes whenever the results could: the text, YouTube Music's
    /// answer, the library and the page on screen.
    fn hits_stamp(&self, pa: &PlayAnything) -> u64 {
        let mut h = DefaultHasher::new();
        pa.query.hash(&mut h);
        pa.generation.hash(&mut h);
        self.pages.search.recent.len().hash(&mut h);
        let view = self.pages.view.target();
        view.key().hash(&mut h);
        for target in library_targets().chain(std::iter::once(view)) {
            if let Some(state) = self.pages.states.get(&target.key()) {
                state.fetched.hash(&mut h);
                state
                    .page
                    .as_ref()
                    .map(|p| p.shelves.iter().map(|s| s.items.len()).sum::<usize>())
                    .hash(&mut h);
            }
        }
        h.finish() | 1
    }

    /// Runs result `index`; `open` (Shift+Enter or Shift+click) opens its
    /// page instead of playing it.
    pub fn choose_hit(
        &mut self,
        index: usize,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pa) = &self.desktop.layers.palette else {
            return;
        };
        let Some(hit) = pa.hits.get(index).cloned() else {
            return;
        };
        log::info!(
            "Play anything: chose {:?} ({}){}",
            hit.title,
            hit.kind.label(),
            if open { ", to open" } else { "" }
        );
        let songs: Vec<Track> = pa
            .hits
            .iter()
            .filter_map(|h| match &h.go {
                Go::Song(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        match hit.go {
            Go::Complete(text) => {
                if let Some(pa) = &self.desktop.layers.palette {
                    pa.input
                        .update(cx, |state, cx| state.set_value(text, window, cx));
                }
                return;
            }
            go => {
                self.close_play_anything(window, cx);
                self.run_hit(go, songs, open, window, cx);
            }
        }
        cx.notify();
    }

    fn run_hit(
        &mut self,
        go: Go,
        songs: Vec<Track>,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match go {
            Go::Song(track) => match (open, track.album.as_ref().and_then(|a| a.target.clone())) {
                (true, Some(album)) => self.open_link(album, cx),
                _ => {
                    let start = songs
                        .iter()
                        .position(|t| t.video_id == track.video_id)
                        .unwrap_or(0);
                    self.send(Command::PlayTracks {
                        tracks: songs,
                        start,
                    });
                }
            },
            Go::Collection { page, play } => match (page, play) {
                (Some(page), _) if open => self.open_link(page, cx),
                (_, Some(play)) => self.activate(play, cx),
                (Some(page), None) => self.play_from_page(page, FromPage::Play, cx),
                (None, None) => {}
            },
            Go::Artist(target) => self.open_link(target, cx),
            Go::Search(query) => self.run_search(query, window, cx),
            Go::Command(cmd) => self.run_command(cmd, window, cx),
            Go::Complete(_) => {}
        }
    }

    fn run_command(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        let playing = self.player.playback.playing;
        match cmd {
            Cmd::Radio(track) => self.send(Command::PlayTarget(control::song_radio(&track))),
            Cmd::Next => self.send(Command::Next),
            Cmd::Pause if playing => self.send(Command::TogglePause),
            Cmd::Play if !playing && !self.player.queue.is_empty() => {
                self.send(Command::TogglePause)
            }
            Cmd::Pause | Cmd::Play => {}
            Cmd::Shuffle => self.send(Command::ToggleShuffle),
            Cmd::Repeat => self.send(Command::CycleRepeat),
            Cmd::Sleep(words) => {
                self.sleep_command(&words, cx);
            }
            Cmd::Eq(words) => {
                self.eq_command(&words, cx);
            }
            Cmd::Mini => self.toggle_mini(cx),
            Cmd::Like => self.toggle_like_current(cx),
            Cmd::Visuals(preset) => {
                let saved = crate::visuals::config::saved();
                crate::visuals::config::set(saved.with_preset(preset), true);
                cx.notify();
            }
            Cmd::Settings(category) => self.open_settings_at(category, window, cx),
        }
    }
}
