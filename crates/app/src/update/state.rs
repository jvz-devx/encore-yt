//! The update area of the app: what the last check found, the download in
//! progress, the settings, and the once-a-day check. The network work runs
//! on the backend's runtime; the results come back here.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use encore_core::paths::Paths;
use gpui_kit::*;

use super::download::{self, Progress};
use super::feed::{self, Release};
use super::handoff;
use super::install::Install;
use super::prefs::Prefs;
use crate::app::MusicApp;

/// At most one automatic check a day; the timer looks every hour, the
/// first time shortly after the start.
const DAY: u64 = 24 * 60 * 60;
const FIRST_LOOK: Duration = Duration::from_secs(20);
const LOOK: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub enum State {
    /// Not checked since the start.
    Idle,
    Checking,
    /// The last check found nothing newer.
    Current,
    Available(Release),
    Downloading {
        version: String,
        progress: Arc<Progress>,
    },
    /// Handed to the helper; the app quits.
    Restarting(String),
    Failed(String),
}

pub struct Updates {
    pub prefs: Prefs,
    pub state: State,
    pub install: Install,
    writer: crate::persistence::Writer,
    /// Not the backend's client: nothing of the YouTube session goes to
    /// GitHub.
    http: Option<reqwest::Client>,
    _timer: Option<Task<()>>,
    _progress: Option<Task<()>>,
}

impl Updates {
    pub(crate) fn flush(&self) {
        self.writer.flush();
    }

    pub fn new(paths: &Paths, executor: &BackgroundExecutor) -> Self {
        let path = paths.config.join("updates.json");
        let http = reqwest::Client::builder()
            .user_agent(concat!("encore-yt/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| log::warn!("update: no HTTP client: {e}"))
            .ok();
        Self {
            prefs: Prefs::load(&path),
            state: State::Idle,
            install: Install::detect(),
            writer: crate::persistence::Writer::new(path, executor),
            http,
            _timer: None,
            _progress: None,
        }
    }

    /// The release on offer, if any.
    pub fn available(&self) -> Option<&Release> {
        match &self.state {
            State::Available(release) => Some(release),
            _ => None,
        }
    }

    /// While downloading: the share received, 0 to 100.
    pub fn percent(&self) -> Option<u64> {
        let State::Downloading { progress, .. } = &self.state else {
            return None;
        };
        let total = progress.total.load(Ordering::Relaxed).max(1);
        Some(progress.received.load(Ordering::Relaxed).min(total) * 100 / total)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

impl MusicApp {
    /// The hourly look that checks once a day while checks are on.
    pub(crate) fn start_update_checks(&mut self, cx: &mut Context<Self>) {
        if self.backend.live().is_none() {
            return;
        }
        log::info!(
            "update: Encore {} ({:?}), pre-releases {}",
            super::VERSION,
            self.updates.install,
            self.updates.prefs.prereleases()
        );
        self.updates._timer = Some(cx.spawn(async move |this, cx| {
            let mut wait = FIRST_LOOK;
            loop {
                cx.background_executor().timer(wait).await;
                wait = LOOK;
                let looked = this.update(cx, |this, cx| {
                    let prefs = &this.updates.prefs;
                    let due = now().saturating_sub(prefs.last_check) >= DAY;
                    if prefs.check
                        && due
                        && matches!(this.updates.state, State::Idle | State::Current)
                    {
                        this.check_for_updates(false, cx);
                    }
                });
                if looked.is_err() {
                    break;
                }
            }
        }));
    }

    /// Asks the feed for a newer release. `manual` (Check now) also reports
    /// a failure; the daily check only logs it.
    pub(crate) fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        let (Some(backend), Some(http)) = (self.backend.live(), self.updates.http.clone()) else {
            return;
        };
        if matches!(
            self.updates.state,
            State::Checking | State::Downloading { .. } | State::Restarting(_)
        ) {
            return;
        }
        let prereleases = self.updates.prefs.prereleases();
        log::info!(
            "update: checking {} (pre-releases {prereleases})",
            feed::url()
        );
        self.updates.state = State::Checking;
        let task = backend
            .runtime
            .spawn(async move { feed::check(&http, super::VERSION, prereleases).await });
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(anyhow::Error::from).and_then(|r| r);
            let _ = this.update(cx, |this, cx| this.on_checked(result, manual, cx));
        })
        .detach();
        cx.notify();
    }

    fn on_checked(
        &mut self,
        result: anyhow::Result<Option<Release>>,
        manual: bool,
        cx: &mut Context<Self>,
    ) {
        self.updates.state = match result {
            Ok(Some(release)) => {
                log::info!("update: {} is available", release.version);
                if !manual {
                    self.notice(format!("Encore {} is available", release.version), cx);
                }
                State::Available(release)
            }
            Ok(None) => {
                log::info!("update: {} is the newest", super::VERSION);
                State::Current
            }
            Err(e) => {
                log::warn!("update: check failed: {e:#}");
                if manual {
                    State::Failed(format!("Couldn't check for updates. {e}"))
                } else {
                    State::Idle
                }
            }
        };
        if !matches!(self.updates.state, State::Failed(_) | State::Idle) {
            self.updates.prefs.last_check = now();
            self.updates.writer.save(&self.updates.prefs);
        }
        cx.notify();
    }

    /// Update and restart: downloads and verifies the release, hands it to
    /// the helper and quits.
    pub(crate) fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(release) = self.updates.available().cloned() else {
            return;
        };
        let (Some(backend), Some(http)) = (self.backend.live(), self.updates.http.clone()) else {
            return;
        };
        let install = self.updates.install.clone();
        let progress = Arc::new(Progress::default());
        self.updates.state = State::Downloading {
            version: release.version.clone(),
            progress: progress.clone(),
        };
        let version = release.version.clone();
        let task = backend.runtime.spawn(async move {
            let job = download::download(&http, &release, &install, progress).await?;
            tokio::task::spawn_blocking(move || {
                let handed = handoff::handoff(&job);
                if handed.is_err() {
                    download::remove(&job.dir);
                }
                handed
            })
            .await?
        });
        cx.spawn(async move |this, cx| {
            let result = task.await.map_err(anyhow::Error::from).and_then(|r| r);
            let _ = this.update(cx, |this, cx| this.on_handed_off(result, version, cx));
        })
        .detach();
        self.updates._progress = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let downloading = this.update(cx, |this, cx| {
                    cx.notify();
                    matches!(this.updates.state, State::Downloading { .. })
                });
                if !matches!(downloading, Ok(true)) {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn on_handed_off(
        &mut self,
        result: anyhow::Result<()>,
        version: String,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(()) => {
                log::info!("update: handed {version} to the helper; quitting");
                self.updates.state = State::Restarting(version);
                cx.notify();
                cx.quit();
            }
            Err(e) => {
                log::warn!("update: {version} not installed: {e:#}");
                self.updates.state = State::Failed(format!("{e}"));
                cx.notify();
            }
        }
    }

    pub(crate) fn set_update_checks(&mut self, on: bool, cx: &mut Context<Self>) {
        self.updates.prefs.check = on;
        self.updates.writer.save(&self.updates.prefs);
        cx.notify();
    }

    pub(crate) fn set_prereleases(&mut self, on: bool, cx: &mut Context<Self>) {
        self.updates.prefs.prereleases = Some(on);
        self.updates.writer.save(&self.updates.prefs);
        // What is on offer depends on it.
        if !matches!(self.updates.state, State::Idle) {
            self.updates.state = State::Idle;
            self.check_for_updates(true, cx);
        }
        cx.notify();
    }

    /// The window is up: a start after an update tells the helper so, and
    /// a rolled back one says what happened.
    pub(crate) fn updates_window_opened(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, |this, _, cx| {
            if let Some(version) = super::acknowledge() {
                this.toast(format!("Updated to Encore {version}"), cx);
            }
            if let Some(error) = super::startup_error() {
                this.error = Some(error);
                cx.notify();
            }
        });
    }
}

/// The first lines of the release notes, without Markdown's marks.
pub fn notes_excerpt(notes: &str, lines: usize) -> String {
    notes
        .lines()
        .map(|line| {
            let line = line.trim().trim_start_matches('#').trim();
            let line = line
                .strip_prefix("* ")
                .or_else(|| line.strip_prefix("- "))
                .map_or_else(|| line.to_string(), |rest| format!("• {rest}"));
            line.replace("**", "")
        })
        .filter(|line| !line.is_empty())
        .take(lines)
        .collect::<Vec<_>>()
        .join("\n")
}
