//! M4: the desktop around the window: MPRIS, tray, single instance and the
//! command line, notifications, signals, the window's lifetime, and keyboard
//! shortcuts.
//!
//! MPRIS, the tray and the command line drive the app through a
//! [`Remote`]: transport goes straight to the backend (it works with no
//! window open), and window or account matters come back here as
//! [`Request`]s, carried out on the foreground.

mod cli;
mod signals;
mod window;

use std::sync::Arc;
use std::sync::mpsc;

use gpui_kit::*;
use ytfast::backend::Backend;
use ytfast::desktop::{Flags, Remote, Request};
use ytfast::paths::Paths;

use crate::app::MusicApp;

pub use cli::command_line;
pub use window::start;

actions!(music, [Quit]);

pub struct Desktop {
    /// Read by MPRIS, the tray and notifications.
    pub flags: Arc<Flags>,
    pub remote: Remote,
    requests: mpsc::Receiver<Request>,
    /// The main window while one is open.
    window: Option<AnyWindowHandle>,
    /// The window's size when it last closed.
    bounds: Option<WindowBounds>,
    /// Follows the open window's focus.
    activation: Option<Subscription>,
    _requests: Task<()>,
}

impl Desktop {
    pub fn new(
        backend: &Backend,
        paths: &Paths,
        _window: &mut Window,
        cx: &mut Context<MusicApp>,
    ) -> (Self, Vec<Subscription>) {
        let flags = Arc::new(Flags::default());
        flags.notifications.store(
            ytfast::settings::Settings::load(paths).notifications,
            std::sync::atomic::Ordering::Relaxed,
        );
        let (request_tx, requests) = mpsc::channel();
        let (wake_tx, wake_rx) = smol::channel::bounded::<()>(1);
        let remote = Remote::new(
            backend.commands(),
            backend.now.clone(),
            request_tx,
            Arc::new(move || {
                let _ = wake_tx.try_send(());
            }),
        );
        start_services(backend, paths, &remote, &flags);
        let task = cx.spawn(async move |this, cx| {
            while wake_rx.recv().await.is_ok() {
                let Some(app) = this.upgrade() else { break };
                cx.update(|cx| handle_requests(&app, cx));
            }
        });
        let desktop = Self {
            flags,
            remote,
            requests,
            window: None,
            bounds: None,
            activation: None,
            _requests: task,
        };
        (desktop, Vec::new())
    }
}

/// The instance socket, the tray, MPRIS (with notifications) and signals.
fn start_services(backend: &Backend, paths: &Paths, remote: &Remote, flags: &Arc<Flags>) {
    let listener = remote.clone();
    if let Err(e) =
        ytfast::single_instance::listen(&paths.runtime, move |message| listener.deliver(message))
    {
        log::warn!("no single instance socket, so the command line can't reach Music: {e}");
    }
    ytfast::tray::start(
        &backend.runtime,
        remote.clone(),
        backend.now.clone(),
        flags.window_open.subscribe(),
    );
    ytfast::mpris::start(
        &backend.runtime,
        remote.clone(),
        backend.now.clone(),
        flags.clone(),
        paths.clone(),
        backend.http.clone(),
    );
    signals::watch(&backend.runtime, remote.clone());
}

/// Carries out the requests from MPRIS, the tray, the command line and
/// signals.
fn handle_requests(app: &Entity<MusicApp>, cx: &mut App) {
    let requests: Vec<Request> = app.read(cx).desktop.requests.try_iter().collect();
    for request in requests {
        match request {
            Request::Show => window::show(app, cx),
            Request::Quit => {
                log::info!("quitting");
                cx.quit();
            }
            Request::Open(link) => window::open_link(app, &link, cx),
            Request::Like => app.update(cx, |this, cx| this.like_playing(cx)),
            // Omarchy's theme hook: the GPUI app has its own theme.
            Request::ReloadThemes => {}
        }
    }
}

impl MusicApp {
    /// `ytfast-gpui like`: likes the playing song or removes its like (M3).
    fn like_playing(&mut self, cx: &mut Context<Self>) {
        self.toggle_like_current(cx);
    }
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("ctrl-q", Quit, Some("Music"))]);
}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, _cx: &mut Context<MusicApp>) -> Div {
    root.on_action(|_: &Quit, _, cx| {
        log::info!("quitting");
        cx.quit();
    })
}
