//! The main window's lifetime. The app (`MusicApp`, with the backend, MPRIS,
//! the tray and the instance socket) outlives its window: closing the window
//! plays on in the background, and a relaunch, `encore-yt show` or the
//! tray opens a new window on the same `MusicApp`, so the page, history and
//! player come back as they were. Wayland can't hide a window, so it is
//! closed and made again on demand.

use std::sync::atomic::Ordering;

use encore_core::desktop::Request;
use encore_core::paths::Paths;
use gpui_kit::*;

use crate::app::{Early, MusicApp};
use crate::nav::View;

/// The app, held here so it lives while no window shows it.
struct Main(Entity<MusicApp>);

impl Global for Main {}

enum Opening {
    First(Paths, Early),
    Again(Entity<MusicApp>),
}

/// Opens the first window and keeps the process running without one.
pub fn start(paths: Paths, early: Early, link: Option<String>, cx: &mut App) {
    // GPUI quits on Linux once the last window closes; this app decides.
    cx.set_quit_mode(QuitMode::Explicit);
    cx.on_window_closed(closed).detach();
    let app = match open(Opening::First(paths, early), cx) {
        Ok(app) => app,
        Err(e) => {
            log::error!("opening the window: {e}");
            cx.quit();
            return;
        }
    };
    crate::startup::mark(crate::startup::Milestone::WindowOpen);
    if let Some(link) = link {
        app.read(cx).desktop.remote.request(Request::Open(link));
    }
    cx.set_global(Main(app));
    cx.activate(true);
}

fn open(opening: Opening, cx: &mut App) -> anyhow::Result<Entity<MusicApp>> {
    let bounds = match &opening {
        Opening::Again(app) => app.read(cx).desktop.bounds,
        Opening::First(..) => None,
    };
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(encore_core::APP_NAME.into()),
            ..Default::default()
        }),
        window_bounds: Some(
            bounds.unwrap_or_else(|| WindowBounds::centered(size(px(1280.), px(820.)), cx)),
        ),
        window_min_size: Some(size(px(900.), px(600.))),
        app_id: Some(encore_core::APP_ID.into()),
        ..Default::default()
    };
    let (handle, app) = gpui_kit::open_window(options, cx, move |window, cx| {
        let app = match opening {
            Opening::First(paths, early) => cx.new(|cx| MusicApp::new(paths, early, window, cx)),
            Opening::Again(app) => app,
        };
        app.update(cx, |this, cx| this.window_opened(window, cx));
        app
    })?;
    app.update(cx, |this, _| this.desktop.window = Some(handle));
    Ok(app)
}

/// Raises the window, or opens it again if it was closed.
pub fn show(app: &Entity<MusicApp>, cx: &mut App) {
    match app.read(cx).desktop.window {
        Some(handle) => {
            let _ = handle.update(cx, |_, window, _| window.activate_window());
        }
        None => {
            log::info!("showing the window again");
            if let Err(e) = open(Opening::Again(app.clone()), cx) {
                log::error!("opening the window: {e}");
            }
        }
    }
}

/// Opens a YouTube Music or YouTube link: its page in the window, or plays it.
pub fn open_link(app: &Entity<MusicApp>, link: &str, cx: &mut App) {
    let Some(target) = encore_core::links::target_from_link(link) else {
        app.update(cx, |this, cx| {
            this.error = Some("That isn't a YouTube Music or YouTube link.".into());
            cx.notify();
        });
        return;
    };
    log::info!("opening a link: {}", target.key());
    let page = View::for_target(&target).is_some();
    app.update(cx, |this, cx| this.activate(target, cx));
    if page {
        show(app, cx);
    }
}

/// A closed main window plays on, unless nothing is queued. Outside Linux
/// there is no tray to bring it back, so closing it quits.
fn closed(cx: &mut App, id: WindowId) {
    let Some(app) = cx.try_global::<Main>().map(|main| main.0.clone()) else {
        return;
    };
    match app.update(cx, |this, _| this.window_closed(id)) {
        Some(true) if cfg!(target_os = "linux") => {
            log::info!("window closed; playing on in the background")
        }
        Some(_) => {
            log::info!("window closed; quitting");
            cx.quit();
        }
        None => {}
    }
}

impl MusicApp {
    /// A window now shows the app: focus its root, catch up on what arrived
    /// while none did, and follow its focus for notifications.
    fn window_opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.desktop.focus_lost = Some(super::keep_focus(window, cx));
        self.drain(Some(window), cx);
        self.updates_window_opened(window, cx);
        let flags = self.desktop.flags.clone();
        flags.window_open.send_replace(true);
        self.desktop.activation =
            Some(cx.observe_window_activation(window, move |_, window, _| {
                flags
                    .focused
                    .store(window.is_window_active(), Ordering::Relaxed);
            }));
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            let now = self.backend.now();
            self.desktop.media = super::media::attach(window, &self.desktop.remote, now, cx);
        }
        // Reopen at the size it had.
        let this = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let bounds = window.window_bounds();
            let _ = this.update(cx, |this, _| this.desktop.bounds = Some(bounds));
            true
        });
    }

    /// `Some(keep running)` when `id` was the main window.
    fn window_closed(&mut self, id: WindowId) -> Option<bool> {
        if self.desktop.window.map(|w| w.window_id()) != Some(id) {
            return None;
        }
        self.desktop.window = None;
        self.desktop.activation = None;
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            self.desktop.media = None;
        }
        self.desktop.flags.window_open.send_replace(false);
        self.desktop.flags.focused.store(false, Ordering::Relaxed);
        Some(!self.player.queue.is_empty())
    }
}
