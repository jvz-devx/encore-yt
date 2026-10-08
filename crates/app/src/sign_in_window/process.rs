//! Own the helper process on one worker thread, including cancellation and reap.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::Duration;

use anyhow::{Context, Result};

const LOOK: Duration = Duration::from_millis(100);
const HELPER: &str = if cfg!(windows) {
    "encore-yt-signin.exe"
} else {
    "encore-yt-signin"
};

pub(crate) struct Helper(SyncSender<()>);

impl Drop for Helper {
    fn drop(&mut self) {
        // Closed means the helper already ended. Only one cancellation is sent.
        let _ = self.0.try_send(());
    }
}

pub(super) struct Outcome {
    pub success: bool,
    pub said: String,
}

pub(super) fn start(
    out: PathBuf,
) -> std::io::Result<(Helper, smol::channel::Receiver<Result<Outcome>>)> {
    let (cancel, cancellation) = mpsc::sync_channel(1);
    let (tx, result) = smol::channel::bounded(1);
    std::thread::Builder::new()
        .name("signin-helper".into())
        .spawn(move || {
            let outcome = Command::new(helper_path())
                .arg("--out")
                .arg(out)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("start sign-in helper")
                .and_then(|child| wait(child, &cancellation));
            // The sheet may have closed while the worker was cleaning up.
            let _ = tx.try_send(outcome);
        })?;
    Ok((Helper(cancel), result))
}

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

struct Reap(Child);

impl Drop for Reap {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(Some(_))) {
            return;
        }
        if let Err(error) = self.0.kill() {
            log::warn!("couldn't stop sign-in helper: {error}");
        }
        if let Err(error) = self.0.wait() {
            log::warn!("couldn't reap sign-in helper: {error}");
        }
    }
}

fn wait(child: Child, cancellation: &Receiver<()>) -> Result<Outcome> {
    let mut child = Reap(child);
    let status = loop {
        if let Some(status) = child.0.try_wait().context("check sign-in helper")? {
            break status;
        }
        match cancellation.recv_timeout(LOOK) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                return Ok(Outcome {
                    success: false,
                    said: String::new(),
                });
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    };
    let mut said = String::new();
    if let Some(stdout) = child.0.stdout.take() {
        // The helper's protocol is one short status line, never cookies.
        stdout
            .take(128)
            .read_to_string(&mut said)
            .context("read sign-in helper status")?;
    }
    Ok(Outcome {
        success: status.success(),
        said,
    })
}

#[cfg(all(test, unix))]
#[allow(
    clippy::unwrap_used,
    reason = "tests launch only synthetic local child processes"
)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_helper_status_after_it_exits() {
        let child = Command::new("sh")
            .args(["-c", "printf 'signed in\\n'"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (_cancel, cancellation) = mpsc::sync_channel(1);
        let outcome = wait(child, &cancellation).unwrap();
        assert!(outcome.success);
        assert_eq!(outcome.said, "signed in\n");
    }

    #[test]
    fn cancellation_reaps_a_running_child() {
        let child = Command::new("sh")
            .args(["-c", "exec sleep 30"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (cancel, cancellation) = mpsc::sync_channel(1);
        cancel.send(()).unwrap();
        let started = std::time::Instant::now();
        let outcome = wait(child, &cancellation).unwrap();
        assert!(!outcome.success);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
