//! The helper: `ytfast-gpui --apply-update <job>`, run from a copy of the
//! old version in the staging folder. It says `ready`, waits for the app
//! to quit (its standard input closes), installs the job, starts the new
//! version with `--update-receipt <job>` and waits for its `started`. Any
//! failure after the app quit puts the previous version back and starts it
//! with `--update-error`. Its output goes to `helper.log` in the folder.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use super::job::{self, Job, Kind};

/// How long the app has to quit, and the new version to open its window.
const APP_EXIT: Duration = Duration::from_secs(60);
const APP_START: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

const RESTORED: &str = "The update couldn't start, so Music went back to the previous version.";

pub fn main(path: &Path) -> i32 {
    close_inherited();
    say(&format!("helper {} for {}", super::VERSION, path.display()));
    match run(path) {
        Ok(()) => 0,
        Err(e) => {
            say(&format!("update failed: {e:#}"));
            1
        }
    }
}

/// The AppImage runtime keeps the old version mounted while any process
/// holds the pipe it hands the app (not close-on-exec); the helper got it
/// from the app and would pass it on to the new version, keeping the old
/// mount alive as long as that runs. Closes every descriptor above the
/// standard three.
#[cfg(target_os = "linux")]
fn close_inherited() {
    use std::os::fd::{FromRawFd, OwnedFd};
    let Ok(entries) = fs::read_dir("/proc/self/fd") else {
        return;
    };
    let fds: Vec<i32> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .filter(|fd| *fd > 2)
        .collect();
    for fd in fds {
        // The listing's own descriptor is closed by now.
        if fs::read_link(format!("/proc/self/fd/{fd}")).is_ok() {
            // SAFETY: nothing in this process owns these yet: the helper
            // runs before any file is opened, and they came from the app.
            drop(unsafe { OwnedFd::from_raw_fd(fd) });
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn close_inherited() {}

fn say(line: &str) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default();
    eprintln!("[{secs:.3}] {line}");
}

fn run(path: &Path) -> Result<()> {
    let job = Job::read(path)?;
    for stale in [job::READY, job::STARTED, job::RESULT, job::PREVIOUS] {
        ensure!(!job.file(stale).exists(), "This update was tried already");
    }
    ensure!(
        job::hash_file(&job.payload)? == job.sha256,
        "The downloaded update changed since it was checked"
    );
    fs::write(job.file(job::READY), b"ready")?;
    wait_for_app()?;
    say("the app quit");
    let exe = match install(&job) {
        Ok(exe) => exe,
        Err(e) => return fail(&job, e, "Update failed"),
    };
    say(&format!(
        "installed {}; starting {}",
        job.version,
        exe.display()
    ));
    if let Err(e) = launch(&exe, path) {
        return fail(&job, e, "Update failed; restored the previous version");
    }
    say(&format!("{} started", job.version));
    fs::write(job.file(job::RESULT), format!("Updated to {}", job.version))?;
    Ok(())
}

/// The app holds the other end of standard input; it closes when the app
/// exits.
fn wait_for_app() -> Result<()> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut sink = [0u8; 64];
        let mut stdin = std::io::stdin();
        while matches!(stdin.read(&mut sink), Ok(n) if n > 0) {}
        let _ = tx.send(());
    });
    rx.recv_timeout(APP_EXIT)
        .context("Music didn't quit within a minute")?;
    // Let the old process finish exiting (its files, its socket).
    std::thread::sleep(Duration::from_millis(300));
    Ok(())
}

/// Puts the new version in place; the executable to start.
fn install(job: &Job) -> Result<PathBuf> {
    let target = &job.target;
    let previous = job.file(job::PREVIOUS);
    match job.kind {
        Kind::AppImage => {
            // A second name for the old file (or a copy), then one rename
            // over it: the path always holds a whole AppImage.
            fs::hard_link(target, &previous)
                .or_else(|_| fs::copy(target, &previous).map(drop))
                .context("Couldn't keep the previous version")?;
            fs::rename(&job.payload, target).context("Couldn't replace the AppImage")?;
            Ok(target.clone())
        }
        Kind::Windows => {
            fs::copy(target, &previous).context("Couldn't keep the previous version")?;
            let dir = target.parent().context("No install folder")?;
            let status = Command::new(&job.payload)
                .args([
                    "/SILENT",
                    "/SUPPRESSMSGBOXES",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                    "/NORESTARTAPPLICATIONS",
                ])
                .arg(format!("/DIR={}", dir.display()))
                .arg(format!("/LOG={}", job.file("installer.log").display()))
                .status()
                .context("Couldn't run the installer")?;
            ensure!(status.success(), "The installer failed ({status})");
            Ok(target.clone())
        }
        Kind::Mac => {
            let new = job.file(job::NEW_APP);
            ensure!(new.is_dir(), "The new app is missing");
            fs::rename(target, job.file("previous.app"))
                .context("Couldn't move the previous version aside")?;
            if let Err(e) = fs::rename(&new, target) {
                fs::rename(job.file("previous.app"), target)?;
                return Err(e).context("Couldn't put the new app in place");
            }
            Ok(mac_exe(target))
        }
    }
}

/// Puts the previous version back.
fn restore(job: &Job) -> Result<()> {
    let target = &job.target;
    match job.kind {
        Kind::AppImage => {
            let previous = job.file(job::PREVIOUS);
            if previous.is_file() {
                fs::rename(&previous, target)?;
            }
        }
        Kind::Windows => {
            let previous = job.file(job::PREVIOUS);
            if previous.is_file() {
                fs::copy(&previous, target)?;
            }
        }
        Kind::Mac => {
            let previous = job.file("previous.app");
            if previous.is_dir() {
                if target.exists() {
                    fs::rename(target, job.file("failed.app"))?;
                }
                fs::rename(&previous, target)?;
            }
        }
    }
    Ok(())
}

fn fail(job: &Job, error: anyhow::Error, what: &str) -> Result<()> {
    say(&format!("{what}: {error:#}"));
    restore(job).context("Couldn't restore the previous version")?;
    fs::write(job.file(job::RESULT), format!("{what}: {error:#}"))?;
    let exe = match job.kind {
        Kind::Mac => mac_exe(&job.target),
        _ => job.target.clone(),
    };
    say(&format!("restored; starting {}", exe.display()));
    let mut command = Command::new(&exe);
    command.arg(super::ERROR_FLAG).arg(RESTORED);
    prepare(&mut command)
        .spawn()
        .context("Couldn't start the previous version")?;
    Err(error)
}

/// Starts the new version with the receipt and waits for its `started`.
fn launch(exe: &Path, receipt: &Path) -> Result<()> {
    let mut command = Command::new(exe);
    command.arg(super::RECEIPT_FLAG).arg(receipt);
    let mut child = prepare(&mut command)
        .spawn()
        .context("Couldn't start the new version")?;
    let started = receipt.with_file_name(job::STARTED);
    let start = Instant::now();
    loop {
        if started.is_file() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            bail!("The new version quit before its window opened ({status})");
        }
        if start.elapsed() > APP_START {
            let _ = child.kill();
            bail!("The new version didn't open its window within a minute");
        }
        std::thread::sleep(POLL);
    }
}

/// A clean start: no AppImage variables from the old mount, and none of
/// its (now gone) directories on PATH.
fn prepare(command: &mut Command) -> &mut Command {
    for var in ["APPIMAGE", "APPDIR", "ARGV0", "OWD"] {
        command.env_remove(var);
    }
    if let Some(path) = std::env::var_os("PATH") {
        let kept: Vec<PathBuf> = std::env::split_paths(&path)
            .filter(|dir| dir.is_dir())
            .collect();
        if let Ok(joined) = std::env::join_paths(kept) {
            command.env("PATH", joined);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}

fn mac_exe(bundle: &Path) -> PathBuf {
    bundle.join("Contents/MacOS/ytfast-gpui")
}
