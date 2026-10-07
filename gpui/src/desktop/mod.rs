//! M4: the desktop around the window: MPRIS, tray, single instance and the
//! command line, notifications, signals, the window's lifetime, keyboard
//! shortcuts (`keys`), context menus (`menu`), Play anything (`palette`) and
//! what they ask of the app (`control`), and the bundled tools (`tools`).
//! On Windows and macOS the system media controls (`media`, M15) stand in
//! for MPRIS.
//!
//! MPRIS, the system media controls, the tray and the command line drive
//! the app through a [`Remote`]: transport goes straight to the backend (it
//! works with no window open), and window or account matters come back here
//! as [`Request`]s, carried out on the foreground.

mod cli;
pub mod control;
mod keys;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod media;
pub mod menu;
pub mod palette;
mod signals;
pub mod tools;
mod window;

use std::sync::Arc;
use std::sync::mpsc;

use gpui_kit::*;
use ytfast::backend::Backend;
use ytfast::desktop::{Flags, Remote, Request};
use ytfast::paths::Paths;

use crate::app::{Link, MusicApp};

pub use cli::command_line;
pub use control::Layers;
pub use keys::{Group, SHORTCUTS, bind_keys, on_actions};
pub use window::start;

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
    /// The system media controls, while the window is open (Windows, macOS).
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    media: Option<media::Media>,
    /// The shortcuts sheet, Play anything, a context menu, a short note.
    pub layers: Layers,
    _requests: Task<()>,
}

impl Desktop {
    pub fn new(
        backend: &Link,
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
            backend.now(),
            request_tx,
            Arc::new(move || {
                let _ = wake_tx.try_send(());
            }),
        );
        // The UI tests run without them (`Link::Fake`).
        if let Some(backend) = backend.live() {
            start_services(backend, paths, &remote, &flags);
        }
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
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            media: None,
            layers: Layers::default(),
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
    // The tray (StatusNotifierItem), MPRIS and notifications are D-Bus
    // services: Linux only.
    #[cfg(target_os = "linux")]
    {
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
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (backend, flags);
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
        }
    }
}

impl MusicApp {
    /// `ytfast-gpui like` and L: likes the playing song or removes its like
    /// (M3).
    pub(crate) fn like_playing(&mut self, cx: &mut Context<Self>) {
        self.toggle_like_current(cx);
    }
}
