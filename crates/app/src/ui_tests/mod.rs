//! Headless UI tests (PLAN M13): the real `MusicApp` in a window of GPUI's
//! test platform, with no desktop, network, audio or D-Bus. The backend is a
//! pair of channels (`Link::Fake`): a test hands the app events, often a
//! page parsed from a saved InnerTube response, clicks and types through
//! GPUI's simulated input, and checks the commands the app sent and what
//! the last frame drew.
//!
//! Elements are found by the names views give them with `debug_selector`
//! (recorded only in test builds); GPUI's test platform has no text
//! shaping or pixels, so a test sees an element's bounds, and its text only
//! where the view puts that in its name. See docs/gpui/GPUI.md "UI tests".

mod home;
mod keys;
mod menu;
mod motion;
mod player;
mod prefetch;
mod search;
mod settings;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use encore_core::backend::{Command, Event};
use encore_core::model::Page;
use encore_core::paths::Paths;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AppContext as _, Bounds, Entity, Modifiers, MouseButton, Pixels, Point, Task, TestAppContext,
    VisualTestContext, WindowBounds, WindowOptions, px, size,
};

use crate::app::{Link, MusicApp};

/// The app's window size (`desktop::window`).
const WINDOW: (f32, f32) = (1280., 820.);

/// The app in a test window, its backend's two ends, and what it sent.
pub struct Ui {
    pub cx: VisualTestContext,
    pub app: Entity<MusicApp>,
    events: mpsc::Sender<Event>,
    commands: tokio::sync::mpsc::UnboundedReceiver<Command>,
    /// Commands taken from the channel and not yet looked at.
    sent: Vec<Command>,
}

impl Ui {
    /// Opens the app the way `desktop::window` does, on a fake backend.
    pub fn start(cx: &mut TestAppContext) -> Self {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init_without_desktop(cx);
            crate::pages::bind_keys(cx);
            crate::playback::bind_keys(cx);
            crate::account::bind_keys(cx);
            crate::desktop::bind_keys(cx);
            crate::extras::bind_keys(cx);
            crate::settings::bind_keys(cx);
        });
        let (command_tx, commands) = tokio::sync::mpsc::unbounded_channel();
        let (events, event_rx) = mpsc::channel();
        let (_, now) = tokio::sync::watch::channel(encore_core::desktop::Now::default());
        let link = Link::Fake {
            commands: command_tx,
            events: event_rx,
            now,
        };
        let paths = scratch_paths();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(WINDOW.0), px(WINDOW.1)),
            })),
            ..Default::default()
        };
        let (handle, app) = cx
            .update(|cx| {
                gpui_kit::open_window(options, cx, move |window, cx| {
                    cx.new(|cx| MusicApp::with_link(link, Task::ready(()), paths, None, window, cx))
                })
            })
            .expect("open the test window");
        let mut ui = Self {
            cx: VisualTestContext::from_window(handle, cx),
            app,
            events,
            commands,
            sent: Vec::new(),
        };
        // Test windows start inactive, and GPUI reports focus changes
        // (a field's Focus event) only in the active window.
        ui.cx.update(|window, _| window.activate_window());
        ui.frame();
        ui
    }

    /// Runs what is due and draws a frame.
    pub fn frame(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.render_frame(cx));
        self.cx.run_until_parked();
    }

    /// Hands the app a backend event, as its event task would, and draws.
    pub fn push(&mut self, event: Event) {
        self.events
            .send(event)
            .expect("the app holds the event channel");
        let app = self.app.clone();
        self.cx
            .update(|window, cx| app.update(cx, |this, cx| this.drain(Some(window), cx)));
        self.frame();
    }

    /// Answers the newest page request for the current view with `page`.
    pub fn answer_page(&mut self, page: Page) {
        let (key, seq) = self.app.read_with(&self.cx, |app, _| {
            let key = app.pages.view.target().key();
            let seq = app.pages.states[&key].seq;
            (key, seq)
        });
        self.push(Event::Page {
            key,
            seq,
            result: Ok(Box::new(page)),
            cached: false,
        });
    }

    /// Everything sent since the last call, oldest first.
    pub fn take_sent(&mut self) -> Vec<Command> {
        while let Ok(command) = self.commands.try_recv() {
            self.sent.push(command);
        }
        std::mem::take(&mut self.sent)
    }

    /// Where the element named `name` was drawn in the last frame.
    pub fn bounds(&mut self, name: &str) -> Option<Bounds<Pixels>> {
        // GPUI's lookup takes a `&'static str`; a test leaks a few names.
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        self.cx.debug_bounds(name)
    }

    /// The element named `name`, which must have been drawn.
    pub fn find(&mut self, name: &str) -> Bounds<Pixels> {
        self.bounds(name)
            .unwrap_or_else(|| panic!("nothing named {name:?} in the last frame"))
    }

    pub fn click(&mut self, name: &str) {
        let at = self.find(name).center();
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.cx.simulate_click(at, Modifiers::none());
        self.frame();
    }

    /// Turns the wheel over the element named `name` by `pixels` (positive
    /// scrolls down).
    pub fn scroll(&mut self, name: &str, pixels: f32) {
        let at = self.find(name).center();
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.cx.simulate_event(gpui_kit::ScrollWheelEvent {
            position: at,
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-pixels))),
            ..Default::default()
        });
        self.frame();
    }

    pub fn right_click(&mut self, name: &str) {
        let at = self.find(name).center();
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.cx
            .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        self.cx
            .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        self.frame();
    }

    /// Keys as GPUI writes them, space separated: "ctrl-, enter".
    pub fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.frame();
    }

    /// A key pressed and let go, as a person does: a focused button or
    /// switch acts on the release (`keys` only presses).
    pub fn press(&mut self, key: &str) {
        self.cx.simulate_keystrokes(key);
        let keystroke = gpui_kit::Keystroke::parse(key).expect("a key");
        self.cx.simulate_event(gpui_kit::KeyUpEvent { keystroke });
        self.frame();
    }

    /// Text typed into the focused field.
    pub fn type_text(&mut self, text: &str) {
        self.cx.simulate_input(text);
        self.frame();
    }

    /// Moves GPUI's test clock on, running the timers that come due.
    pub fn wait(&mut self, duration: std::time::Duration) {
        self.cx.executor().advance_clock(duration);
        self.frame();
    }
}

/// A page parsed from a saved signed-out InnerTube response in the core
/// crate's `tests/fixtures/innertube/`.
pub fn fixture(name: &str) -> Page {
    let path = format!(
        "{}/../core/tests/fixtures/innertube/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let value: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    encore_core::parse::page(&value)
}

/// Directories of this test alone, never made: nothing of the real
/// profile (cookies, settings, cache) is read, and saves fail quietly.
fn scratch_paths() -> Paths {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "encore-yt-ui-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    Paths {
        config: root.join("config"),
        cache: root.join("cache"),
        runtime: root.join("runtime"),
    }
}
