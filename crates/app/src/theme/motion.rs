//! Motion: durations, the one easing curve, and the settings that scale
//! and switch them (Settings → Motion, `~/.config/encore-yt/motion.json`).
//! Hover and press states change at once; motion is for things that
//! appear or move.
//!
//! Every animation goes through [`animate`] (or asks [`scaled`] for its
//! duration), so the speed, the per-kind switches and reduced motion apply
//! everywhere. The settings live on the UI thread and are read while
//! drawing, also by recipes that draw without a context (skeletons).

mod config;

use std::cell::RefCell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::*;

pub use config::{Align, Anchor, Config, Lyrics, PageStyle, Reduce, TextSize};

/// Small state changes: icons swapping, a toggle, menus opening.
pub const FAST: Duration = Duration::from_millis(120);
/// Page content arriving, panels opening.
pub const BASE: Duration = Duration::from_millis(200);
/// Larger moves: a panel sliding, the cover flying, a lyric line growing.
pub const SLOW: Duration = Duration::from_millis(320);

/// Ease out (quint): starts fast, settles softly.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}

/// What is moving, for the per-kind switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Pages,
    Menus,
    Panels,
    Toasts,
    NowPlaying,
    Skeleton,
    Lyrics,
}

struct State {
    config: Config,
    /// Where the settings are saved; `None` in tests.
    writer: Option<crate::persistence::Writer>,
    /// The desktop asks for less motion.
    desktop: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        config: Config::default(),
        writer: None,
        desktop: false,
    });
}

/// Loads the saved settings. Call before the theme starts following the
/// desktop.
pub fn init(path: PathBuf, cx: &mut App) {
    let config = Config::load(&path);
    STATE.with_borrow_mut(|s| {
        s.config = config;
        s.writer = Some(crate::persistence::Writer::new(
            path,
            cx.background_executor(),
        ));
    });
}

/// The settings in effect.
pub fn config() -> Config {
    STATE.with_borrow(|s| s.config)
}

pub(crate) fn flush() {
    STATE.with_borrow(|state| {
        if let Some(writer) = &state.writer {
            writer.flush();
        }
    });
}

/// Changes the settings: saves them, applies reduced motion and redraws.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Config)) {
    STATE.with_borrow_mut(|s| {
        change(&mut s.config);
        if let Some(writer) = &s.writer {
            writer.save(&s.config);
        }
    });
    apply_reduced(cx);
    cx.refresh_windows();
}

/// The desktop's motion preference changed (theme follows the portal).
pub fn follow_desktop(reduced: bool, cx: &mut App) {
    STATE.with_borrow_mut(|s| s.desktop = reduced);
    apply_reduced(cx);
}

/// Whether motion is cut back now: the setting, or the desktop's wish
/// when the setting follows the system.
pub fn reduced() -> bool {
    STATE.with_borrow(|s| match s.config.reduce {
        Reduce::System => s.desktop,
        Reduce::Always => true,
        Reduce::Never => false,
    })
}

/// The desktop's own preference, for Settings to say what "System" means.
pub fn desktop_reduced() -> bool {
    STATE.with_borrow(|s| s.desktop)
}

/// GPUI's flag follows ours, so every `with_animation` and the kit's
/// spinners settle at once too.
fn apply_reduced(cx: &mut App) {
    cx.set_reduce_motion(reduced());
}

/// Whether `kind` moves at all.
pub fn enabled(kind: Kind) -> bool {
    let c = config();
    if reduced() || c.speed <= 0.0 {
        return false;
    }
    match kind {
        Kind::Pages => c.pages != PageStyle::None,
        Kind::Menus => c.menus,
        Kind::Panels => c.panels,
        Kind::Toasts => c.toasts,
        Kind::NowPlaying => c.now_playing,
        Kind::Skeleton => c.skeleton,
        Kind::Lyrics => c.lyrics.glide,
    }
}

/// `base` at the chosen speed, or `None` when `kind` doesn't move.
pub fn scaled(kind: Kind, base: Duration) -> Option<Duration> {
    enabled(kind).then(|| base.div_f32(config().speed))
}

/// `base` at the chosen speed for motion that has no switch of its own
/// (a carousel gliding), or `None` when motion is off or instant.
pub fn duration(base: Duration) -> Option<Duration> {
    let speed = config().speed;
    (!reduced() && speed > 0.0).then(|| base.div_f32(speed))
}

/// How far a motion of `kind` that began at `started` has got, eased, or
/// `None` once it has arrived (or when `kind` doesn't move). For motion a
/// view drives itself, frame by frame.
pub fn progress(kind: Kind, base: Duration, started: Instant) -> Option<f32> {
    let duration = scaled(kind, base)?;
    let t = started.elapsed().as_secs_f32() / duration.as_secs_f32();
    (t < 1.0).then(|| ease_out(t))
}

/// `base` at a speed, for Settings to show; `None` at instant.
pub fn at_speed(base: Duration, speed: f32) -> Option<Duration> {
    (speed > 0.0).then(|| base.div_f32(speed))
}

/// `el` animated from `t = 0` to `1` over `base` at the chosen speed with
/// [`ease_out`], or drawn at its last frame when `kind` doesn't move.
pub fn animate<E: IntoElement + 'static>(
    el: E,
    id: impl Into<ElementId>,
    kind: Kind,
    base: Duration,
    animator: impl Fn(E, f32) -> E + 'static,
) -> AnyElement {
    match scaled(kind, base) {
        Some(duration) => el
            .with_animation(id, Animation::new(duration).with_easing(ease_out), animator)
            .into_any_element(),
        None => animator(el, 1.0).into_any_element(),
    }
}

/// [`animate`] as a method: `el.with_motion(id, kind, base, |el, t| ..)`.
pub trait MotionExt: IntoElement + Sized + 'static {
    fn with_motion(
        self,
        id: impl Into<ElementId>,
        kind: Kind,
        base: Duration,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnyElement {
        animate(self, id, kind, base, animator)
    }
}

impl<E: IntoElement + 'static> MotionExt for E {}

/// Settings for a test, without a file.
#[cfg(test)]
pub fn set_for_test(config: Config) {
    STATE.with_borrow_mut(|s| {
        s.config = config;
        s.writer = None;
    });
}

/// The desktop's wish, for a test.
#[cfg(test)]
pub fn set_desktop_for_test(reduced: bool) {
    STATE.with_borrow_mut(|s| s.desktop = reduced);
}
