//! M18: signing in from the app. The sign-in sheet offers three routes:
//! sign in with the browser (open YouTube Music, then look through the
//! browser profiles every few seconds until one is signed in), import a
//! cookies.txt (the desktop's file picker), or paste the Cookie header.
//! The backend saves imported and pasted cookies as a private cookie file
//! and connects; this module follows it until the account is checked.
//! `views::account::sign_in` draws the sheet.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use encore_core::auth::{BrowserScan, Profile};
use encore_core::backend::Command;
use encore_core::browsers::{self, Browser, Plan};
use encore_core::model::Account;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::app::MusicApp;

/// YouTube Music, opened in the browser for the browser route.
pub const MUSIC_URL: &str = "https://music.youtube.com";
/// How often the browser route looks for a signed-in profile.
const POLL: Duration = Duration::from_secs(3);
/// How long it keeps looking.
const WAIT: Duration = Duration::from_secs(180);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Choose,
    Browser,
    File,
    Paste,
    /// The sign-in window (M31), where its helper is installed.
    Window,
}

pub enum Step {
    Idle,
    /// The browser route: YouTube Music is open in the browser and the
    /// profiles are checked every few seconds. `checked` is the last look.
    Waiting {
        started: Instant,
        checked: Option<Vec<String>>,
        /// Why a browser other than the default was opened.
        note: Option<String>,
    },
    /// The browser route can't work here: no installed browser can be read.
    CantRead,
    /// The browser route gave up after [`WAIT`].
    TimedOut {
        checked: Vec<String>,
    },
    /// Saving an imported or pasted cookie file.
    Saving,
    /// A sign-in was found; YouTube Music is being asked who it is.
    Connecting {
        source: String,
    },
    Failed(String),
}

pub struct SignIn {
    pub open: bool,
    pub route: Route,
    pub step: Step,
    /// The Cookie header (masked) and a cookies file's path, made in the
    /// window that shows the sheet.
    pub paste: Option<Entity<InputState>>,
    pub path: Option<Entity<InputState>>,
    poll: Option<Task<()>>,
    /// What the browser route found on this system; tests set it.
    pub(crate) browsers: Option<browsers::Report>,
    /// The browser the route opened instead of the default, to open again.
    open_in: Option<Browser>,
    /// The sign-in window's process, killed when this is dropped.
    pub(crate) window: Option<crate::sign_in_window::Helper>,
    /// The cookie file the window wrote, removed once the backend saved it.
    pub(crate) window_file: Option<PathBuf>,
    _subscriptions: Vec<Subscription>,
}

impl SignIn {
    pub fn new() -> Self {
        Self {
            open: false,
            route: Route::Choose,
            step: Step::Idle,
            paste: None,
            path: None,
            poll: None,
            browsers: None,
            open_in: None,
            window: None,
            window_file: None,
            _subscriptions: Vec::new(),
        }
    }

    /// Whether the app should offer Sign in first: nobody is signed in and
    /// no browser profile or cookie file holds a sign-in to reconnect with.
    pub fn needed(account: &Account, profiles: &[Profile]) -> bool {
        matches!(account, Account::SignedOut { .. }) && profiles.is_empty()
    }

    pub(crate) fn set_poll(&mut self, task: Task<()>) {
        self.poll = Some(task);
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.step,
            Step::Waiting { .. } | Step::Saving | Step::Connecting { .. }
        )
    }
}

impl MusicApp {
    pub fn open_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.account.menu = false;
        self.account.settings = false;
        let paste = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Paste the Cookie header here")
        });
        let path = cx.new(|cx| InputState::new(window, cx).placeholder("~/Downloads/cookies.txt"));
        let subscriptions = vec![
            cx.subscribe_in(&paste, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.paste_cookies(cx);
                }
            }),
            cx.subscribe_in(&path, window, |this, _, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.import_typed_path(cx);
                }
            }),
        ];
        self.sign_in = SignIn {
            open: true,
            paste: Some(paste),
            path: Some(path),
            _subscriptions: subscriptions,
            ..SignIn::new()
        };
        self.account.focus.focus(window, cx);
        cx.notify();
    }

    pub fn close_sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sign_in = SignIn::new();
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Back to the three routes, stopping whatever the open one was doing.
    pub fn sign_in_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sign_in.poll = None;
        self.sign_in.window = None;
        self.sign_in.route = Route::Choose;
        self.sign_in.step = Step::Idle;
        self.account.focus.focus(window, cx);
        cx.notify();
    }

    pub fn sign_in_route(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.sign_in.route = route;
        self.sign_in.step = Step::Idle;
        match route {
            Route::Browser => self.sign_in_with_browser(cx),
            Route::Paste => focus(self.sign_in.paste.as_ref(), window, cx),
            Route::Window => self.sign_in_with_window(cx),
            Route::File | Route::Choose => self.account.focus.focus(window, cx),
        }
        cx.notify();
    }

    /// Opens YouTube Music in a browser Encore can read and looks for a
    /// signed-in profile now and every few seconds, for a few minutes. Where
    /// no installed browser can be read it says so at once instead.
    pub fn sign_in_with_browser(&mut self, cx: &mut Context<Self>) {
        let report = self
            .sign_in
            .browsers
            .clone()
            .unwrap_or_else(browsers::detect);
        let (open, note) = match browsers::plan(&report) {
            Plan::Unreadable { .. } => {
                self.sign_in.poll = None;
                self.sign_in.open_in = None;
                self.sign_in.step = Step::CantRead;
                cx.notify();
                return;
            }
            Plan::Wait { open, note } => (open, note),
        };
        self.sign_in.open_in = open;
        self.open_music(cx);
        self.sign_in.step = Step::Waiting {
            started: Instant::now(),
            checked: None,
            note,
        };
        self.send(Command::ScanBrowsers);
        self.sign_in.poll = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let more = this.update(cx, |this, cx| this.poll_browsers(cx));
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        }));
        cx.notify();
    }

    /// Opens YouTube Music in the browser the route picked, else the
    /// system's.
    pub(crate) fn open_music(&self, cx: &mut Context<Self>) {
        let opened = self
            .sign_in
            .open_in
            .as_ref()
            .is_some_and(|browser| !cfg!(test) && browsers::open_in(browser, MUSIC_URL));
        if !opened {
            cx.open_url(MUSIC_URL);
        }
    }

    /// One tick of the browser route; false once it stops.
    fn poll_browsers(&mut self, cx: &mut Context<Self>) -> bool {
        let Step::Waiting {
            started, checked, ..
        } = &self.sign_in.step
        else {
            return false;
        };
        if started.elapsed() >= WAIT {
            self.sign_in.step = Step::TimedOut {
                checked: checked.clone().unwrap_or_default(),
            };
            cx.notify();
            return false;
        }
        self.send(Command::ScanBrowsers);
        true
    }

    pub(crate) fn on_browser_scan(&mut self, scan: BrowserScan, _cx: &mut Context<Self>) {
        let Step::Waiting { checked, .. } = &mut self.sign_in.step else {
            return;
        };
        *checked = Some(scan.checked);
        if let Some(profile) = scan.signed_in.into_iter().next() {
            self.sign_in.poll = None;
            self.sign_in.step = Step::Connecting {
                source: profile.label,
            };
            self.account.profile = Some(profile.id.clone());
            self.account.account = Account::Checking;
            self.send(Command::UseProfile(profile.id));
        }
    }

    /// Import a cookies file: the desktop's file picker.
    pub fn pick_cookie_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = picked.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        this.import_cookies(path, cx);
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(error)) => {
                    log::warn!("file picker: {error:#}");
                    this.sign_in.step = Step::Failed(
                        "The file picker didn't open. Type the file's path instead.".into(),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn import_typed_path(&mut self, cx: &mut Context<Self>) {
        let Some(typed) = self.sign_in.path.as_ref().map(|p| p.read(cx).value()) else {
            return;
        };
        let typed = typed.trim();
        if typed.is_empty() {
            return;
        }
        let path = match typed.strip_prefix("~/") {
            Some(rest) => std::env::home_dir().unwrap_or_default().join(rest),
            None => PathBuf::from(typed),
        };
        self.import_cookies(path, cx);
    }

    pub(crate) fn import_cookies(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.sign_in.step = Step::Saving;
        self.send(Command::ImportCookies(path));
        cx.notify();
    }

    pub fn paste_cookies(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.sign_in.paste.as_ref().map(|p| p.read(cx).value()) else {
            return;
        };
        if text.trim().is_empty() {
            return;
        }
        self.sign_in.step = Step::Saving;
        self.send(Command::PasteCookies(text.to_string()));
        cx.notify();
    }

    pub(crate) fn on_cookies_saved(
        &mut self,
        saved: Result<Profile, String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = self.sign_in.window_file.take() {
            cx.background_spawn(async move {
                if let Err(error) = std::fs::remove_file(file)
                    && error.kind() != std::io::ErrorKind::NotFound
                {
                    log::warn!("couldn't remove sign-in helper output: {error}");
                }
            })
            .detach();
        }
        if !matches!(self.sign_in.step, Step::Saving) {
            return;
        }
        self.sign_in.step = match saved {
            Ok(profile) => {
                self.account.profile = Some(profile.id);
                self.account.account = Account::Checking;
                Step::Connecting {
                    source: profile.label,
                }
            }
            Err(error) => Step::Failed(error),
        };
    }

    /// Follows the account check a route started: signed in closes the
    /// sheet, anything else shows why.
    pub(crate) fn sign_in_on_account(&mut self) {
        if !self.sign_in.open {
            return;
        }
        match &self.account.account {
            Account::SignedIn { .. } if self.sign_in.busy() => self.sign_in = SignIn::new(),
            Account::SignedOut { reason } | Account::Unverified { reason }
                if matches!(self.sign_in.step, Step::Connecting { .. }) =>
            {
                self.sign_in.step = Step::Failed(reason.clone());
            }
            _ => {}
        }
    }
}

fn focus(input: Option<&Entity<InputState>>, window: &mut Window, cx: &mut App) {
    if let Some(input) = input {
        input.update(cx, |input, cx| input.focus(window, cx));
    }
}
