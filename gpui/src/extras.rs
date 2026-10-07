//! M6: equalizer, sleep timer, loudness levelling, smooth mixes, audition,
//! the most-replayed ridge, Stage and the mini player.
//!
//! The state lives here and the behaviour in the submodules; the views are
//! in `views::extras`. Other areas (Play anything, menus, Settings) reach
//! these through the `pub(crate)` methods on [`MusicApp`]:
//!
//! - [`MusicApp::sleep`] / [`MusicApp::sleep_command`] (`sleep 30`, `sleep end`)
//! - [`MusicApp::eq_preset`] / [`MusicApp::eq_command`] (`eq bass`, `eq off`)
//! - [`MusicApp::toggle_mini`] (`mini`), [`MusicApp::toggle_stage`],
//!   [`MusicApp::toggle_equalizer`], [`MusicApp::jump_to_peak`]
//! - [`MusicApp::set_normalize`], [`MusicApp::set_mixes`] (Settings)

mod audition;
mod heat;
mod mini;
mod sound;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::equalizer::Equalizer;
use ytfast::heat::Heat;
use ytfast::model::Mixes;

use crate::app::MusicApp;

pub use audition::Shown;
pub use mini::MiniPlayer;

actions!(
    extras,
    [
        ToggleStage,
        StageFullscreen,
        ToggleEqualizer,
        JumpToPeak,
        ToggleMini,
        ClosePanel
    ]
);

pub struct Extras {
    /// Most-replayed heat by video id, asked once per song.
    pub heat: HashMap<String, Option<Heat>>,
    heat_requested: HashSet<String>,
    /// The equalizer panel is open (E).
    pub equalizer_open: bool,
    /// The sleep timer's menu is open.
    pub sleep_open: bool,
    /// The panels' focus, so Esc closes them before anything else.
    pub panel_focus: FocusHandle,
    /// The equalizer as edited here, shown until playback reports catch up.
    eq_edit: Option<(Equalizer, Instant)>,
    /// The band being dragged, and the one under the pointer.
    pub eq_band: Option<usize>,
    pub eq_hover: Option<usize>,
    /// The equalizer graph's bounds, as last painted.
    pub eq_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// When a click outside closed a panel: the click that follows on its
    /// button must not open it again.
    panel_closed_at: Option<Instant>,
    /// Redraws the sleep timer's countdown while one is set.
    pub sleep_tick: Option<Task<()>>,
    pub stage: Stage,
    pub(crate) hold: audition::Hold,
    /// The mini player's window while it is open.
    mini: Option<AnyWindowHandle>,
    /// Settings → Smooth mixes: the crossfade length slider, and the length
    /// shown while it is dragged (sent to the backend on release).
    pub mix_length: Entity<SliderState>,
    pub mix_drag: Option<u8>,
}

/// Stage: the cover and large lyrics fill the window.
#[derive(Default)]
pub struct Stage {
    pub open: bool,
    /// Stage made the window full screen (F11); leaving Stage ends it.
    pub fullscreen: bool,
    pub scroll: ScrollHandle,
    /// The song the lyrics were laid out for; a new song starts at the top.
    pub song: Option<String>,
    /// A redraw due when the next line starts, and that line's index.
    pub wake: Option<(usize, Task<()>)>,
}

impl Extras {
    pub fn new(_window: &mut Window, cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        let mix_length = cx.new(|_| {
            SliderState::new()
                .min(f32::from(Mixes::SHORTEST))
                .max(f32::from(Mixes::LONGEST))
                .step(1.)
                .default_value(f32::from(Mixes::default().seconds))
        });
        let subscriptions =
            vec![cx.subscribe(
                &mix_length,
                |this, _, event: &SliderEvent, cx| match event {
                    SliderEvent::Change(value) => {
                        this.extras.mix_drag = Some(value.start().round() as u8);
                        cx.notify();
                    }
                    SliderEvent::Release(value) => {
                        this.extras.mix_drag = None;
                        let seconds = value.start().round() as u8;
                        let mixes = this.player.playback.mixes;
                        if seconds != mixes.seconds {
                            this.set_mixes(Mixes { seconds, ..mixes }, cx);
                        }
                    }
                },
            )];
        (
            Self {
                heat: HashMap::new(),
                heat_requested: HashSet::new(),
                equalizer_open: false,
                sleep_open: false,
                panel_focus: cx.focus_handle(),
                eq_edit: None,
                eq_band: None,
                eq_hover: None,
                eq_bounds: Rc::default(),
                panel_closed_at: None,
                sleep_tick: None,
                stage: Stage::default(),
                hold: audition::Hold::default(),
                mini: None,
                mix_length,
                mix_drag: None,
            },
            subscriptions,
        )
    }
}

impl MusicApp {
    /// Once per frame of the main window: asks for the playing song's heat,
    /// publishes the audition for rows and cards, and lets go of an
    /// audition when the window loses the focus.
    pub(crate) fn extras_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request_heat();
        cx.set_global(Shown(self.player.playback.audition.clone()));
        if !window.is_window_active() {
            self.audition_release(cx);
        }
    }

    pub fn toggle_stage(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.extras.stage.open;
        self.extras.stage.open = open;
        self.extras.equalizer_open = false;
        self.extras.sleep_open = false;
        if open {
            self.extras.stage.song = None;
            self.request_current_lyrics();
        } else if std::mem::take(&mut self.extras.stage.fullscreen) && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        log::info!("stage {}", if open { "opened" } else { "closed" });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// F11 inside Stage: the window to full screen and back.
    pub fn stage_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.extras.stage.open {
            return;
        }
        let on = !window.is_fullscreen();
        self.extras.stage.fullscreen = on;
        window.toggle_fullscreen();
        log::info!("stage full screen {on}");
        cx.notify();
    }

    pub fn toggle_equalizer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.extras.equalizer_open;
        if self.extras.stage.open || (open && self.just_closed()) {
            return;
        }
        self.extras.equalizer_open = open;
        self.extras.sleep_open = false;
        self.extras.eq_band = None;
        self.focus_panel(open, window, cx);
    }

    pub fn toggle_sleep_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let open = !self.extras.sleep_open;
        if open && self.just_closed() {
            return;
        }
        self.extras.sleep_open = open;
        self.extras.equalizer_open = false;
        self.focus_panel(open, window, cx);
    }

    /// Closes the equalizer and the sleep menu.
    pub fn close_panels(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.extras.equalizer_open = false;
        self.extras.sleep_open = false;
        self.focus_panel(false, window, cx);
    }

    /// A click outside closed the panels: they close, and remember when.
    pub fn close_panels_outside(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.extras.panel_closed_at = Some(Instant::now());
        self.close_panels(window, cx);
    }

    fn just_closed(&self) -> bool {
        self.extras
            .panel_closed_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(300))
    }

    fn focus_panel(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open {
            window.focus(&self.extras.panel_focus, cx);
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }
}

/// Shortcuts for this area: in the "Music" key context (not while typing in
/// a field), in Stage, and in the open panel.
pub fn bind_keys(cx: &mut App) {
    const MUSIC: &str = "Music && !Input && !MusicMenu";
    cx.bind_keys([
        KeyBinding::new("f", ToggleStage, Some(MUSIC)),
        KeyBinding::new("e", ToggleEqualizer, Some(MUSIC)),
        KeyBinding::new("p", JumpToPeak, Some(MUSIC)),
        KeyBinding::new("ctrl-m", ToggleMini, Some("Music")),
        KeyBinding::new("escape", ClosePanel, Some("ExtrasPanel")),
        KeyBinding::new("e", ClosePanel, Some("ExtrasPanel")),
        KeyBinding::new("f", ToggleStage, Some("Stage")),
        KeyBinding::new("escape", ToggleStage, Some("Stage")),
        KeyBinding::new("f11", StageFullscreen, Some("Stage")),
    ]);
}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, cx: &mut Context<MusicApp>) -> Div {
    root.on_action(cx.listener(|this, _: &ToggleStage, window, cx| this.toggle_stage(window, cx)))
        .on_action(
            cx.listener(|this, _: &StageFullscreen, window, cx| this.stage_fullscreen(window, cx)),
        )
        .on_action(
            cx.listener(|this, _: &ToggleEqualizer, window, cx| this.toggle_equalizer(window, cx)),
        )
        .on_action(cx.listener(|this, _: &JumpToPeak, _, cx| this.jump_to_peak(cx)))
        .on_action(cx.listener(|this, _: &ToggleMini, _, cx| this.toggle_mini(cx)))
        .on_action(cx.listener(|this, _: &ClosePanel, window, cx| this.close_panels(window, cx)))
        .map(|root| audition::listen_root(root, cx))
}
