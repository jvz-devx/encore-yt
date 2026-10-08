//! From a verified download to a helper waiting for the app to quit: checks
//! what can be checked while the old version still runs (the new AppImage
//! and the new macOS app answer `--version` with the release's version),
//! writes the job, starts the helper from a copy of this program and waits
//! for it to say `ready`. Blocking; run it off the UI thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};

use super::job::{self, Job, Kind};

const HELPER_READY: Duration = Duration::from_secs(20);
const PROBE: Duration = Duration::from_secs(30);

/// The helper's standard input. The helper reads it until it closes,
/// which happens when this process exits, however it exits.
static PIPE: Mutex<Option<ChildStdin>> = Mutex::new(None);

/// Prepares `job` and starts its helper. The app must quit once this
/// returns: the helper waits a minute for that.
pub fn handoff(job: &Job) -> Result<()> {
    prepare(job)?;
    let path = job.write()?;
    let helper = copy_helper(job)?;
    let log = fs::File::create(job.file(job::HELPER_LOG))?;
    let mut command = Command::new(&helper);
    command
        .arg(super::APPLY_FLAG)
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(log.try_clone()?)
        .stderr(log);
    detach(&mut command);
    let mut child = command
        .spawn()
        .context("Couldn't start the update helper")?;
    if let Ok(mut pipe) = PIPE.lock() {
        *pipe = child.stdin.take();
    }
    wait_ready(job, &mut child)?;
    log::info!("update: helper {} is ready", child.id());
    Ok(())
}

fn prepare(job: &Job) -> Result<()> {
    match job.kind {
        Kind::AppImage => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&job.payload, fs::Permissions::from_mode(0o755))?;
            }
            probe(&job.payload, &job.version)
        }
        Kind::Mac => {
            let app = mac_extract(job)?;
            probe(&app.join("Contents/MacOS/encore-yt"), &job.version)
        }
        // A setup program can't be asked; it is run by the helper.
        Kind::Windows => Ok(()),
    }
}

/// Runs `exe --version`: it must start and name `version`.
pub fn probe(exe: &Path, version: &str) -> Result<()> {
    let mut child = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("The new version doesn't start")?;
    let start = Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > PROBE {
            stop_child(&mut child);
            bail!("The new version doesn't answer");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output()?;
    let said = String::from_utf8_lossy(&output.stdout);
    ensure!(
        said.trim() == format!("encore-yt {version}"),
        "The download isn't Encore {version} (it says {:?})",
        said.trim()
    );
    Ok(())
}

/// macOS: copies `Encore.app` out of the disk image to `new.app` and checks
/// it is this app.
fn mac_extract(job: &Job) -> Result<PathBuf> {
    let mount = job.file("mnt");
    fs::create_dir(&mount)?;
    run(Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(&job.payload))
    .context("Couldn't open the disk image")?;
    let copied = (|| {
        let app = fs::read_dir(&mount)?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .context("The disk image has no app")?;
        let new = job.file(job::NEW_APP);
        run(Command::new("ditto").arg(&app).arg(&new)).context("Couldn't copy the new app")?;
        Ok::<_, anyhow::Error>(new)
    })();
    if let Err(error) = run(Command::new("hdiutil").arg("detach").arg(&mount)) {
        log::warn!("couldn't detach update disk image: {error:#}");
    }
    let new = copied?;
    let id = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(new.join("Contents/Info.plist"))
        .output()?;
    ensure!(
        String::from_utf8_lossy(&id.stdout).trim() == "io.github.jvz-devx.encore-yt",
        "The disk image holds another app"
    );
    match Command::new("xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&new)
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => log::warn!("couldn't clear update quarantine: {status}"),
        Err(error) => log::warn!("couldn't clear update quarantine: {error}"),
    }
    Ok(new)
}

/// Windows locks a running executable, and the AppImage's executable is
/// gone once the app quits, so the helper runs from a copy: of the
/// executable, or on macOS of the whole bundle (a signed executable copied
/// out of its bundle won't start).
fn copy_helper(job: &Job) -> Result<PathBuf> {
    if job.kind == Kind::Mac {
        let bundle = job.file("helper.app");
        run(Command::new("ditto").arg(&job.target).arg(&bundle))
            .context("Couldn't prepare the update helper")?;
        return Ok(bundle.join("Contents/MacOS/encore-yt"));
    }
    let helper = job.file(if cfg!(windows) {
        "helper.exe"
    } else {
        "helper"
    });
    fs::copy(std::env::current_exe()?, &helper).context("Couldn't prepare the update helper")?;
    Ok(helper)
}

/// The helper outlives the app: its own process group on Unix (a closed
/// terminal doesn't take it along), no console window on Windows.
fn detach(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
}

fn wait_ready(job: &Job, child: &mut Child) -> Result<()> {
    let ready = job.file(job::READY);
    let start = Instant::now();
    while !ready.exists() {
        if child.try_wait()?.is_some() {
            bail!(
                "The update helper stopped. See {}",
                job.file(job::HELPER_LOG).display()
            );
        }
        if start.elapsed() > HELPER_READY {
            stop_child(child);
            bail!("The update helper didn't start. Try again.");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(())
}

pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .with_context(|| format!("couldn't run {:?}", command.get_program()))?;
    ensure!(
        status.success(),
        "{:?} failed: {status}",
        command.get_program()
    );
    Ok(())
}

pub(super) fn stop_child(child: &mut Child) {
    if let Err(error) = child.kill() {
        log::warn!("couldn't stop update child: {error}");
    }
    if let Err(error) = child.wait() {
        log::warn!("couldn't reap update child: {error}");
    }
}

#[cfg(all(test, unix))]
#[allow(
    clippy::unwrap_used,
    reason = "tests launch only a synthetic local child"
)]
mod tests {
    use super::*;

    #[test]
    fn stopped_children_are_reaped() {
        let mut child = Command::new("sh")
            .args(["-c", "exec sleep 30"])
            .spawn()
            .unwrap();
        stop_child(&mut child);
        assert!(child.try_wait().unwrap().is_some());
    }
}
