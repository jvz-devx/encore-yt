//! The window's state and the bridge to the backend.
//!
//! The backend runs on its own tokio runtime and reports through a std
//! channel plus a wake callback. The callback pokes an async channel; a
//! foreground task awaits it and drains the events into this entity, so the
//! app redraws only when something happened. When only the position moved
//! (most playback reports, and a slow clock while a song plays), just the
//! views that show it redraw (`MusicApp::position_moved`).
//!
//! Each area of the app keeps its state and its event handlers in its own
//! module, so work on one area doesn't touch another's files:
//!
//! - `pages`: views, history, page cache, search (M1)
//! - `playback`: queue, player bar, Up next, Now Playing, lyrics (M2)
//! - `account`: sign-in state, profiles, account edits, likes (M3)
//! - `desktop`: MPRIS, tray, single instance, notifications, keys (M4)
//! - `extras`: equalizer, sleep timer, mixes, audition, heat, Stage (M6)

use std::time::Duration;

use gpui_kit::*;
use ytfast::backend::{Backend, Command, Event};
use ytfast::paths::Paths;

pub use crate::link::Link;

use crate::account::AccountUi;
use crate::desktop::Desktop;
use crate::extras::Extras;
use crate::nav::View;
use crate::pages::Pages;
use crate::playback::Player;
use crate::sidebar::Sidebar;

/// How often the player bar's position moves while a song plays.
const POSITION_TICK: Duration = Duration::from_millis(500);

pub struct MusicApp {
    pub backend: Link,
    #[allow(
        dead_code,
        reason = "settings and the desktop modules read it (M3, M4)"
    )]
    pub paths: Paths,
    pub pages: Pages,
    pub player: Player,
    pub account: AccountUi,
    pub desktop: Desktop,
    pub extras: Extras,
    pub sidebar: Sidebar,
    /// The newest error, shown in the strip under the top bar.
    pub error: Option<String>,
    /// The root's focus, so shortcuts in the "Music" key context reach the
    /// root while no field has the focus.
    pub focus: FocusHandle,
    /// The newest playback report that came while no window was open; the
    /// player's sliders need a window to move.
    pending_playback: Option<ytfast::model::Playback>,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
    _clock: Task<()>,
}

impl MusicApp {
    pub fn new(paths: Paths, early: Early, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let Started { backend, wake } = early.join();
        crate::startup::mark(crate::startup::Milestone::Backend);
        // Not tied to a window: the app outlives its window (desktop).
        let events = cx.spawn(async move |this, cx| {
            while wake.recv().await.is_ok() {
                let Some(app) = this.upgrade() else { break };
                cx.update(|cx| drain_events(&app, cx));
            }
        });
        Self::with_link(
            Link::Live(backend),
            events,
            paths,
            Some(HOME_SEQ),
            window,
            cx,
        )
    }

    /// The app on `backend`; `events` drains what it reports (the UI tests
    /// drain by hand). `home_asked` is the request for Home sent already.
    pub(crate) fn with_link(
        backend: Link,
        events: Task<()>,
        paths: Paths,
        home_asked: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let clock = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POSITION_TICK).await;
                let ticked = this.update(cx, |this, cx| {
                    if this.player.playback.playing {
                        this.position_moved(cx);
                    }
                });
                if ticked.is_err() {
                    break;
                }
            }
        });

        let mut subscriptions = vec![
            // Save the session and stop mpv before the process ends; the
            // backend's Drop may not run on quit.
            cx.on_app_quit(|this, _| {
                this.backend.shutdown();
                async {}
            }),
        ];
        let (pages, subs) = Pages::new(window, cx);
        subscriptions.extend(subs);
        let (player, subs) = Player::new(window, cx);
        subscriptions.extend(subs);
        let (account, subs) = AccountUi::new(window, cx);
        subscriptions.extend(subs);
        let (desktop, subs) = Desktop::new(&backend, &paths, window, cx);
        subscriptions.extend(subs);
        let (extras, subs) = Extras::new(window, cx);
        subscriptions.extend(subs);

        let sidebar = Sidebar::new(&paths.cache);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut app = Self {
            backend,
            paths,
            pages,
            player,
            account,
            desktop,
            extras,
            sidebar,
            // A missing mpv, yt-dlp or deno, from the start.
            error: crate::desktop::tools::missing_notice(),
            focus,
            pending_playback: None,
            _subscriptions: subscriptions,
            _events: events,
            _clock: clock,
        };
        match home_asked {
            Some(seq) => app.page_requested(View::Home.target(), seq),
            None => app.ensure_page(app.pages.view.target(), false),
        }
        crate::startup::mark(crate::startup::Milestone::AppBuilt);
        app
    }

    pub fn send(&mut self, command: Command) {
        self.sidebar_sent(&command);
        self.backend.send(command);
    }

    /// Takes in the backend's events; `window` is the one showing the app,
    /// if any. Reports that only move the position redraw just what shows
    /// it, not the whole app.
    pub(crate) fn drain(&mut self, mut window: Option<&mut Window>, cx: &mut Context<Self>) {
        let mut changed = false;
        if let Some(window) = window.as_deref_mut()
            && let Some(playback) = self.pending_playback.take()
        {
            changed |= self.on_playback(playback, window, cx);
        }
        while let Some(event) = self.backend.next_event() {
            changed |= self.handle(event, window.as_deref_mut(), cx);
        }
        if changed {
            cx.notify();
        } else {
            self.position_moved(cx);
        }
    }

    /// Hands each event to the module that owns it; whether the app's
    /// state changed beyond the position.
    fn handle(
        &mut self,
        event: Event,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> bool {
        match event {
            Event::Playback(playback) => {
                return match window {
                    Some(window) => self.on_playback(playback, window, cx),
                    None => {
                        self.pending_playback = Some(playback);
                        true
                    }
                };
            }
            Event::Account(account) => self.on_account(account, cx),
            Event::Page {
                key,
                seq,
                result,
                cached,
            } => self.on_page(key, seq, result, cached),
            Event::More {
                key,
                shelf,
                token,
                result,
            } => self.on_more(key, shelf, token, result),
            Event::Suggestions { input, items } => self.on_suggestions(input, items),
            Event::Lyrics { id, result } => self.on_lyrics(id, result),
            Event::Searches(saved) => self.on_searches(saved),
            Event::Queue(queue) => self.on_queue(queue),
            Event::Error(error) => {
                log::warn!("{error}");
                self.error = Some(error);
            }
            Event::Profiles { list, current } => self.on_profiles(list, current),
            Event::Channels(channels) => self.on_channels(channels),
            Event::AccountEdited { op, result } => self.on_account_edited(op, result, cx),
            Event::Likes(likes) => self.on_likes(likes),
            Event::AccountRefresh(targets) => self.on_account_refresh(targets),
            Event::Heat { id, heat } => self.on_heat(id, heat),
            Event::QuickResults { query, result } => self.on_quick_results(query, result),
        }
        true
    }
}

/// The backend, started on a thread of its own from `main` before the
/// platform (M17), so restoring the session, reading the cookies, checking
/// the account and loading Home (its saved copy, then a fresh one) overlap
/// the start of the platform and the window. The window's first frame then
/// shows the saved Home.
pub struct Early(std::thread::JoinHandle<anyhow::Result<Started>>);

struct Started {
    backend: Backend,
    /// Poked each time the backend has events.
    wake: smol::channel::Receiver<()>,
}

/// The request number of the first Home request, sent by [`Early`].
const HOME_SEQ: u64 = 1;

impl Early {
    pub fn start(paths: Paths) -> std::io::Result<Self> {
        let thread = std::thread::Builder::new().name("ytfast-start".into());
        thread
            .spawn(move || {
                let (wake_tx, wake) = smol::channel::bounded::<()>(1);
                let backend = Backend::start(paths, move || {
                    // Full means a wake is pending already; that drain catches up.
                    let _ = wake_tx.try_send(());
                })?;
                backend.send(Command::Page {
                    target: View::Home.target(),
                    seq: HOME_SEQ,
                });
                Ok(Started { backend, wake })
            })
            .map(Self)
    }

    fn join(self) -> Started {
        match self.0.join() {
            Ok(Ok(started)) => started,
            Ok(Err(e)) => panic!("starting the backend: {e:#}"),
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

/// Drains in the window that shows the app, or without one while it's closed.
fn drain_events(app: &Entity<MusicApp>, cx: &mut App) {
    let drained = cx.with_window(app.entity_id(), |window, cx| {
        app.update(cx, |this, cx| this.drain(Some(window), cx));
    });
    if drained.is_none() {
        app.update(cx, |this, cx| this.drain(None, cx));
    }
}

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::startup::rendered(self, window);
        // The app's views sit in a cached view between the effects layers
        // (see `visuals`), so an effects frame doesn't re-render them.
        crate::visuals::shell(self, window, cx)
    }
}
