//! Startup milestones (M17): one `startup: <milestone> at <ms> ms` log line
//! each, timed from the top of `main`, so a launch's log shows where its
//! first second went. docs/gpui/STARTUP.md has the numbers.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::*;

use crate::app::MusicApp;
use crate::nav::View;

/// When `main` started, and the wall clock then (for timing the exec).
static START: OnceLock<(Instant, u128)> = OnceLock::new();
/// The milestones logged so far, one bit each.
static SEEN: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, Debug)]
pub enum Milestone {
    Logging,
    Platform,
    Theme,
    Backend,
    AppBuilt,
    WindowOpen,
    FirstFrameDrawn,
    FirstFramePresented,
    CachedHomePresented,
    FreshHomePresented,
    AccountChecked,
}

impl Milestone {
    fn name(self) -> &'static str {
        match self {
            Self::Logging => "logging ready",
            Self::Platform => "platform ready",
            Self::Theme => "fonts and theme ready",
            Self::Backend => "backend started",
            Self::AppBuilt => "app state built",
            Self::WindowOpen => "window open",
            Self::FirstFrameDrawn => "first frame drawn",
            Self::FirstFramePresented => "first frame presented",
            Self::CachedHomePresented => "Home from cache presented",
            Self::FreshHomePresented => "Home fresh presented",
            Self::AccountChecked => "account checked",
        }
    }

    fn bit(self) -> u32 {
        1 << self as u32
    }
}

/// The frame milestones; once all are seen, rendering stops checking.
const FRAMES: u32 = (1 << Milestone::FirstFrameDrawn as u32)
    | (1 << Milestone::FirstFramePresented as u32)
    | (1 << Milestone::CachedHomePresented as u32)
    | (1 << Milestone::FreshHomePresented as u32);

/// Starts the clock; the first thing `main` does.
pub fn begin() {
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let _ = START.set((Instant::now(), epoch));
}

/// Logs `milestone` the first time it is reached.
pub fn mark(milestone: Milestone) {
    let Some((start, epoch)) = START.get() else {
        return;
    };
    if SEEN.fetch_or(milestone.bit(), Ordering::Relaxed) & milestone.bit() != 0 {
        return;
    }
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    if let Milestone::Logging = milestone {
        log::info!("startup: main at unix {epoch} ms");
    }
    log::info!("startup: {} at {ms:.0} ms", milestone.name());
}

fn seen(milestone: Milestone) -> bool {
    SEEN.load(Ordering::Relaxed) & milestone.bit() != 0
}

/// Logs `milestone` once the frame being drawn is on screen.
fn after_frame(milestone: Milestone, window: &Window) {
    if !seen(milestone) {
        window.on_next_frame(move |_, _| mark(milestone));
    }
}

/// Called from the app's render: the frame milestones.
pub fn rendered(app: &MusicApp, window: &Window) {
    if SEEN.load(Ordering::Relaxed) & FRAMES == FRAMES {
        return;
    }
    mark(Milestone::FirstFrameDrawn);
    after_frame(Milestone::FirstFramePresented, window);
    if app.pages.view != View::Home {
        return;
    }
    let home = app.pages.states.get(&View::Home.target().key());
    match home.filter(|state| state.page.is_some()) {
        Some(state) if state.cached => after_frame(Milestone::CachedHomePresented, window),
        Some(_) => after_frame(Milestone::FreshHomePresented, window),
        None => {}
    }
}
