//! `ENCORE_FRAME_LOG=<ms>` logs every window frame whose UI thread
//! work (the shell's render through the last layer's paint: render,
//! layout, prepaint and paint, not GPUI's GPU submission) took longer than
//! that many milliseconds. For measuring hitches such as an effect's first
//! use.

use std::cell::Cell;
use std::time::Instant;

use gpui_kit::*;

thread_local! {
    static STARTED: Cell<Option<Instant>> = const { Cell::new(None) };
    /// Milliseconds spent this frame setting up an effect (device,
    /// pipelines, textures).
    static SETUP: Cell<f64> = const { Cell::new(0.0) };
}

/// The threshold in milliseconds, if frames are logged.
fn threshold() -> Option<f64> {
    static THRESHOLD: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *THRESHOLD.get_or_init(|| {
        std::env::var("ENCORE_FRAME_LOG")
            .ok()
            .and_then(|v| v.parse().ok())
    })
}

/// Marks the start of a frame (the shell's render).
pub fn frame_started() {
    if threshold().is_some() {
        STARTED.with(|s| s.set(Some(Instant::now())));
    }
}

/// Counts the time since `started` as effect set-up in this frame; such
/// a frame is logged whatever its length.
pub fn setup(started: Instant) {
    if threshold().is_some() {
        SETUP.with(|s| s.set(s.get() + started.elapsed().as_secs_f64() * 1000.0));
    }
}

/// The shell's last child: logs the frame once it has painted.
pub fn frame_end() -> Option<impl IntoElement> {
    threshold()?;
    Some(
        canvas(
            |_, _, _| (),
            |_, (), _, _| {
                let (Some(started), Some(limit)) = (STARTED.with(Cell::take), threshold()) else {
                    return;
                };
                let ms = started.elapsed().as_secs_f64() * 1000.0;
                let setup = SETUP.with(|s| s.replace(0.0));
                if setup > 0.0 {
                    log::info!("visuals: frame {ms:.1} ms (effect set-up {setup:.1} ms)");
                } else if ms > limit {
                    log::info!("visuals: frame {ms:.1} ms");
                }
            },
        )
        .absolute()
        .size_0(),
    )
}
