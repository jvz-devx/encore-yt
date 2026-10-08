//! M31 spike: the sign-in window. `encore-yt-signin` (crates/signin) opens
//! YouTube Music in the OS's own web engine, waits for the session cookies
//! and writes them as a cookie file; this starts it, waits for it to end and
//! imports the file like "Import a cookies file". Offered wherever the
//! helper is installed (the Windows and macOS installers ship it), and the
//! first choice on Windows, where the browser route can't read Edge, Chrome
//! or Brave (docs/gpui/SIGNIN-WINDOW.md).

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

mod process;
pub(crate) use process::Helper;

use gpui_kit::*;

use crate::app::MusicApp;
use crate::sign_in::{Route, Step};

/// Whether the Sign in sheet offers the window: when the helper is
/// installed. `ENCORE_SIGNIN_WINDOW=0` hides it, `=1` offers it regardless.
pub fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| match std::env::var_os("ENCORE_SIGNIN_WINDOW") {
        Some(v) if v == "0" => false,
        Some(v) if v == "1" => true,
        _ => process::helper_path().is_file(),
    })
}

/// Whether the window comes first on the Sign in sheet: on Windows, where
/// the browser route can only read Firefox.
pub fn suggested() -> bool {
    cfg!(windows) && enabled()
}

impl MusicApp {
    /// Starts the helper and follows it until the window closes.
    pub fn sign_in_with_window(&mut self, cx: &mut Context<Self>) {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let out = std::env::temp_dir().join(format!(
            "encore-yt-signin-{}-{stamp}-cookies.txt",
            std::process::id()
        ));
        let (helper, ended) = match process::start(out.clone()) {
            Ok(started) => started,
            Err(error) => {
                log::warn!("sign-in window: {error}");
                self.sign_in.step =
                    Step::Failed("The sign-in window isn't part of this install.".into());
                return;
            }
        };
        self.sign_in.window = Some(helper);
        self.sign_in.window_file = Some(out.clone());
        self.sign_in.step = Step::Waiting {
            started: Instant::now(),
            checked: None,
            note: None,
        };
        self.sign_in.set_poll(cx.spawn(async move |this, cx| {
            let (ok, said) = match ended.recv().await {
                Ok(Ok(outcome)) => (outcome.success, outcome.said),
                Ok(Err(error)) => {
                    log::warn!("sign-in window: {error:#}");
                    (false, String::new())
                }
                Err(error) => {
                    log::warn!("sign-in helper worker stopped: {error}");
                    (false, String::new())
                }
            };
            let _ = this.update(cx, |this, cx| this.on_window_ended(ok, &said, out, cx));
        }));
        cx.notify();
    }

    fn on_window_ended(&mut self, ok: bool, said: &str, file: PathBuf, cx: &mut Context<Self>) {
        if self.sign_in.route != Route::Window || !matches!(self.sign_in.step, Step::Waiting { .. })
        {
            return;
        }
        self.sign_in.window = None;
        match (ok, said.trim()) {
            (true, "signed in") => self.import_cookies(file, cx),
            (true, _) => {
                self.sign_in.route = Route::Choose;
                self.sign_in.step = Step::Idle;
                self.sign_in.window_file = None;
                cx.notify();
            }
            _ => {
                self.sign_in.window_file = None;
                self.sign_in.step =
                    Step::Failed("The sign-in window stopped. Try another way to sign in.".into());
                cx.notify();
            }
        }
    }
}
