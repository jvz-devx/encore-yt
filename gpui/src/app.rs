//! The window's state and the bridge to the backend.
//!
//! The backend runs on its own tokio runtime and reports through a std
//! channel plus a wake callback. The callback pokes an async channel; a
//! foreground task awaits it and drains the events into this entity, so the
//! app redraws only when something happened (and on a slow clock while a
//! song plays, for the position).
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

use crate::account::AccountUi;
use crate::desktop::Desktop;
use crate::extras::Extras;
use crate::pages::Pages;
use crate::playback::Player;

/// How often the player bar's position moves while a song plays.
const POSITION_TICK: Duration = Duration::from_millis(500);

pub struct MusicApp {
    pub backend: Backend,
    #[allow(
        dead_code,
        reason = "settings and the desktop modules read it (M3, M4)"
    )]
    pub paths: Paths,
    pub pages: Pages,
    pub player: Player,
    pub account: AccountUi,
    #[allow(dead_code, reason = "filled by M4")]
    pub desktop: Desktop,
    pub extras: Extras,
    /// The newest error, shown in the strip under the top bar.
    pub error: Option<String>,
    /// The root's focus, so shortcuts in the "Music" key context reach the
    /// root while no field has the focus.
    pub focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    _events: Task<()>,
    _clock: Task<()>,
}

impl MusicApp {
    pub fn new(paths: Paths, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (wake_tx, wake_rx) = smol::channel::bounded::<()>(1);
        let backend = match Backend::start(paths.clone(), move || {
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
                    if this.player.playback.playing {
                        cx.notify();
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
            error: None,
            focus,
            _subscriptions: subscriptions,
            _events: events,
            _clock: clock,
        };
        app.ensure_page(app.pages.view.target(), false);
        app
    }

    pub fn send(&mut self, command: Command) {
        self.backend.send(command);
    }

    fn drain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Ok(event) = self.backend.events.try_recv() {
            self.handle(event, window, cx);
        }
        cx.notify();
    }

    /// Hands each event to the module that owns it.
    fn handle(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
        match event {
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
            Event::Playback(playback) => self.on_playback(playback, window, cx),
            Event::Error(error) => {
                log::warn!("{error}");
                self.error = Some(error);
            }
            Event::Profiles { list, current } => self.on_profiles(list, current),
            Event::AccountEdited { op, result } => self.on_account_edited(op, result, cx),
            Event::Likes(likes) => self.on_likes(likes),
            Event::AccountRefresh(targets) => self.on_account_refresh(targets),
            Event::Heat { id, heat } => self.on_heat(id, heat),
            Event::QuickResults { query, result } => self.on_quick_results(query, result),
        }
    }
}

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::views::root(self, window, cx)
    }
}
