//! M31 spike: the sign-in window. `encore-yt-signin` (crates/signin) opens
//! YouTube Music in the OS's own web engine, waits for the session cookies
//! and writes them as a cookie file; this starts it, waits for it to end and
//! imports the file like "Import a cookies file". Offered only with
//! `ENCORE_SIGNIN_WINDOW=1` until the spike is judged (docs/gpui/SIGNIN-WINDOW.md).

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::app::MusicApp;
use crate::sign_in::{Route, Step};

const HELPER: &str = if cfg!(windows) {
    "encore-yt-signin.exe"
} else {
    "encore-yt-signin"
};
/// How often the helper is looked at.
const LOOK: Duration = Duration::from_millis(500);

/// Whether the Sign in sheet offers the window.
pub fn enabled() -> bool {
    std::env::var_os("ENCORE_SIGNIN_WINDOW").is_some_and(|v| v == "1")
}

/// The running helper; dropping it closes its window.
pub struct Helper(Arc<Mutex<Child>>);

impl Drop for Helper {
    fn drop(&mut self) {
        if let Ok(mut child) = self.0.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `ENCORE_SIGNIN_BIN`, else the helper next to this executable.
fn helper_path() -> PathBuf {
    std::env::var_os("ENCORE_SIGNIN_BIN")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join(HELPER)))
        })
        .unwrap_or_else(|| PathBuf::from(HELPER))
}

impl MusicApp {
    /// Starts the helper and follows it until the window closes.
    pub fn sign_in_with_window(&mut self, cx: &mut Context<Self>) {
        let out = std::env::temp_dir().join(format!(
            "encore-yt-signin-{}-cookies.txt",
            std::process::id()
        ));
        let spawned = Command::new(helper_path())
            .arg("--out")
            .arg(&out)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(error) => {
                log::warn!("sign-in window: {error}");
                self.sign_in.step =
                    Step::Failed("The sign-in window isn't part of this install.".into());
                return;
            }
        };
        let stdout = child.stdout.take();
        let child = Arc::new(Mutex::new(child));
        self.sign_in.window = Some(Helper(child.clone()));
        self.sign_in.window_file = Some(out.clone());
        self.sign_in.step = Step::Waiting {
            started: Instant::now(),
            checked: None,
            note: None,
        };
        self.sign_in.set_poll(cx.spawn(async move |this, cx| {
            let status = loop {
                cx.background_executor().timer(LOOK).await;
                let looked = child.lock().map(|mut c| c.try_wait());
                match looked {
                    Ok(Ok(Some(status))) => break Some(status),
                    Ok(Ok(None)) => {}
                    _ => break None,
                }
            };
            let said = stdout
                .map(|mut out| {
                    let mut text = String::new();
                    let _ = std::io::Read::read_to_string(&mut out, &mut text);
                    text
                })
                .unwrap_or_default();
            let ok = status.is_some_and(|s| s.success());
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
