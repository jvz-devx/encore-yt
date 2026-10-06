//! The window's state and the bridge to the backend.
//!
//! The backend runs on its own tokio runtime and reports through a std
//! channel plus a wake callback. The callback pokes an async channel; a
//! foreground task awaits it and drains the events into this entity, so the
//! app redraws only when something happened (and on a slow clock while a
//! song plays, for the position).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;
use ytfast::backend::{Backend, Command, Event};
use ytfast::model::{Account, Item, Playback, Shelf, Target, Track};
use ytfast::paths::Paths;

use crate::nav::{PageState, View};

/// How often the player bar's position moves while a song plays.
const POSITION_TICK: Duration = Duration::from_millis(500);
/// A page older than this is fetched again when it's opened.
const STALE: Duration = Duration::from_secs(300);
/// The seek slider's scale: per mille of the song.
pub const SEEK_SCALE: f32 = 1000.0;

pub struct MusicApp {
    pub backend: Backend,
    pub account: Account,
    pub view: View,
    pub history: Vec<View>,
    pub forward: Vec<View>,
    pub pages: HashMap<String, PageState>,
    page_seq: u64,
    pub queue: Vec<Track>,
    pub playback: Playback,
    /// When `playback` arrived; the shown position moves on from there.
    playback_at: Instant,
    pub error: Option<String>,
    pub search: Entity<InputState>,
    pub seek: Entity<SliderState>,
    pub volume: Entity<SliderState>,
    /// The seek slider is held: playback updates don't move it.
    seeking: bool,
    /// The volume slider shows the restored volume (set once, then the
    /// slider leads).
    volume_synced: bool,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
    _clock: Task<()>,
}

impl MusicApp {
    pub fn new(paths: Paths, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (wake_tx, wake_rx) = smol::channel::bounded::<()>(1);
        let backend = match Backend::start(paths, move || {
            // Full means a wake is pending already; that drain catches up.
            let _ = wake_tx.try_send(());
        }) {
            Ok(backend) => backend,
            Err(e) => panic!("starting the backend: {e:#}"),
        };
        let events = cx.spawn_in(window, async move |this, cx| {
            while wake_rx.recv().await.is_ok() {
                let drained = this.update_in(cx, |this, window, cx| this.drain(window, cx));
                if drained.is_err() {
                    break;
                }
            }
        });
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POSITION_TICK).await;
                let ticked = this.update(cx, |this, cx| {
                    if this.playback.playing {
                        cx.notify();
                    }
                });
                if ticked.is_err() {
                    break;
                }
            }
        });

        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search songs, albums, artists, podcasts")
        });
        let seek = cx.new(|_| SliderState::new().min(0.).max(SEEK_SCALE));
        let volume = cx.new(|_| SliderState::new().min(0.).max(100.).default_value(100.));
        let subscriptions = vec![
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
            cx.subscribe(&seek, |this, _, event: &SliderEvent, _| match event {
                SliderEvent::Change(_) => this.seeking = true,
                SliderEvent::Release(value) => {
                    this.seeking = false;
                    let to = f64::from(value.start() / SEEK_SCALE) * this.playback.duration;
                    this.backend.send(Command::Seek(to));
                }
            }),
            // Save the session and stop mpv before the process ends; the
            // backend's Drop may not run on quit.
            cx.on_app_quit(|this, _| {
                this.backend.shutdown();
                async {}
            }),
            cx.subscribe(&volume, |this, _, event: &SliderEvent, _| {
                let (SliderEvent::Change(value) | SliderEvent::Release(value)) = event;
                this.backend.send(Command::Volume(f64::from(value.start())));
            }),
        ];

        let mut app = Self {
            backend,
            account: Account::Checking,
            view: View::Home,
            history: Vec::new(),
            forward: Vec::new(),
            pages: HashMap::new(),
            page_seq: 0,
            queue: Vec::new(),
            playback: Playback::default(),
            playback_at: Instant::now(),
            error: None,
            search,
            seek,
            volume,
            seeking: false,
            volume_synced: false,
            _subscriptions: subscriptions,
            _events: events,
            _clock: clock,
        };
        app.ensure_page(app.view.target(), false);
        app
    }

    /// Where the song is now: the last reported position, moved on by the
    /// time since while it plays.
    pub fn position(&self) -> f64 {
        let mut position = self.playback.position;
        if self.playback.playing && !self.playback.loading {
            position += self.playback_at.elapsed().as_secs_f64();
        }
        if self.playback.duration > 0.0 {
            position = position.min(self.playback.duration);
        }
        position
    }

    pub fn current(&self) -> Option<&Track> {
        self.playback.index.and_then(|i| self.queue.get(i))
    }

    pub fn send(&mut self, command: Command) {
        self.backend.send(command);
    }

    pub fn open(&mut self, view: View, cx: &mut Context<Self>) {
        if view != self.view {
            let previous = std::mem::replace(&mut self.view, view);
            self.history.push(previous);
            self.forward.clear();
            if self.history.len() > 50 {
                self.history.remove(0);
            }
        }
        self.ensure_page(self.view.target(), false);
        cx.notify();
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.history.pop() {
            let left = std::mem::replace(&mut self.view, view);
            self.forward.push(left);
            self.ensure_page(self.view.target(), false);
            cx.notify();
        }
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.forward.pop() {
            let left = std::mem::replace(&mut self.view, view);
            self.history.push(left);
            self.ensure_page(self.view.target(), false);
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
        let needed = match self.pages.get(&key) {
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
        self.page_seq += 1;
        let seq = self.page_seq;
        let state = self.pages.entry(key).or_insert_with(|| PageState {
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

    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(event) = self.backend.events.try_recv() {
            self.handle(event, window, cx);
        }
        cx.notify();
    }

    fn handle(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            Event::Account(account) => {
                let was = matches!(self.account, Account::SignedIn { .. });
                let now = matches!(account, Account::SignedIn { .. });
                self.account = account;
                if was != now {
                    // Home, Library and the rest change with the account.
                    self.ensure_page(self.view.target(), true);
                }
            }
            Event::Page {
                key,
                seq,
                result,
                cached,
            } => {
                let Some(state) = self.pages.get_mut(&key) else {
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
            Event::Queue(queue) => self.queue = queue,
            Event::Playback(playback) => {
                if !self.seeking && playback.duration > 0.0 {
                    let at = (playback.position / playback.duration) as f32 * SEEK_SCALE;
                    self.seek
                        .update(cx, |slider, cx| slider.set_value(at, window, cx));
                }
                if !self.volume_synced {
                    self.volume_synced = true;
                    let volume = playback.volume as f32;
                    self.volume
                        .update(cx, |slider, cx| slider.set_value(volume, window, cx));
                }
                self.playback = playback;
                self.playback_at = Instant::now();
            }
            Event::Error(error) => {
                log::warn!("{error}");
                self.error = Some(error);
            }
            // Later milestones (docs/gpui/PLAN.md): continuations, search
            // suggestions, lyrics, account edits, profiles, heat.
            _ => {}
        }
    }
}

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::views::root(self, window, cx)
    }
}
