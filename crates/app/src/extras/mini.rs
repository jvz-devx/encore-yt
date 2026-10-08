//! The mini player: a small second window with the cover, the song, its
//! progress and the transport (Ctrl+M, or `mini` in Play anything). It
//! drives the same session; its expand button brings back the full window.

use gpui_kit::*;

use crate::app::MusicApp;

/// The mini player's view: it reads the app and redraws when it changes,
/// or when only the position moved (`playback::Clock`).
pub struct MiniPlayer {
    pub app: Entity<MusicApp>,
    _observe: [Subscription; 2],
}

impl MiniPlayer {
    pub fn new(app: Entity<MusicApp>, cx: &mut Context<Self>) -> Self {
        let clock = app.read(cx).player.clock.clone();
        let observe = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&clock, |_, _, cx| cx.notify()),
        ];
        Self {
            app,
            _observe: observe,
        }
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::views::extras::mini::render(self, window, cx)
    }
}

impl MusicApp {
    /// Opens the mini player, or closes it if it is open.
    pub(crate) fn toggle_mini(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.extras.mini.take()
            && handle
                .update(cx, |_, window, _| window.remove_window())
                .is_ok()
        {
            log::info!("mini player closed");
            cx.notify();
            return;
        }
        let app = cx.entity();
        // Opened once this update is over: its first frame reads the app.
        cx.defer(move |cx| match open(app.clone(), cx) {
            Ok(handle) => {
                log::info!("mini player opened");
                app.update(cx, |this, cx| {
                    this.extras.mini = Some(handle);
                    cx.notify();
                });
            }
            Err(e) => log::warn!("opening the mini player: {e}"),
        });
    }
}

fn open(app: Entity<MusicApp>, cx: &mut App) -> anyhow::Result<AnyWindowHandle> {
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some(format!("{} mini player", encore_core::APP_NAME).into()),
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: crate::views::extras::mini::SIZE,
        })),
        window_min_size: Some(crate::views::extras::mini::MIN_SIZE),
        app_id: Some(format!("{}.mini", encore_core::app_id())),
        ..Default::default()
    };
    let (handle, _) = gpui_kit::open_window(options, cx, move |_, cx| {
        cx.new(|cx| MiniPlayer::new(app, cx))
    })?;
    Ok(handle)
}
