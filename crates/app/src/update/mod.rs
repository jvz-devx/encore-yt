//! M16: updates from GitHub Releases.
//!
//! The app reads the repository's release list at most once a day (and from
//! Settings on demand), offers the newest version above its own, and
//! installs it the way this copy was installed ([`install`]):
//!
//! - **AppImage:** the new AppImage is downloaded next to the running one,
//!   checked against the release's `checksums.txt`, asked for its
//!   `--version`, and renamed over the old file (one atomic rename), with
//!   the old one kept as `previous` for rollback.
//! - **Windows installer:** the setup program is downloaded, checked, and
//!   run with `/SILENT` into the same folder.
//! - **macOS:** the disk image is downloaded and checked, its `Encore.app`
//!   copied out and asked for its `--version`, and the bundle swapped.
//! - **.deb, .rpm, the portable zip and source builds** only say that a
//!   new version is out and how to get it.
//!
//! The install itself is done by a helper ([`helper`]): a copy of this
//! program started with `--apply-update <job>`. It waits for the app to
//! quit, replaces it, starts the new version with `--update-receipt <job>`
//! and waits for that one to write `started` once its window is up. If it
//! doesn't within a minute, or exits first, the helper puts the old
//! version back and starts it with `--update-error <message>`.
//!
//! Adapted from fastframe's `fastframe-update` (github.com/crmne/fastframe,
//! MIT): the check, the installation kinds, verifying before handing off,
//! the helper and its `ready`/`started` files, and the rollback. This app's
//! assets, its unsigned macOS app and its AppImage need their own rules,
//! so it doesn't use that crate directly.

mod download;
mod feed;
mod handoff;
mod helper;
mod install;
mod job;
mod prefs;
mod state;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Mutex;

pub use state::{State, Updates, notes_excerpt};

/// This build's version: the release's (CI sets `ENCORE_VERSION` from the
/// tag, `0.1.0-alpha.2`), else the crate's.
pub const VERSION: &str = match option_env!("ENCORE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

const APPLY_FLAG: &str = "--apply-update";
const RECEIPT_FLAG: &str = "--update-receipt";
const ERROR_FLAG: &str = "--update-error";

/// What the update flags on the command line asked of this start.
#[derive(Default)]
struct Startup {
    /// This start finishes an update: the job to acknowledge.
    receipt: Option<PathBuf>,
    /// An update failed and was rolled back: what to tell the user.
    error: Option<String>,
}

static STARTUP: Mutex<Startup> = Mutex::new(Startup {
    receipt: None,
    error: None,
});

/// First thing in `main`: runs the helper (and exits) for
/// `--apply-update`, keeps `--update-receipt` and `--update-error` for
/// later, and returns the remaining arguments.
pub fn intercept() -> Vec<String> {
    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_some_and(|a| a == APPLY_FLAG) {
        let code = match args.get(1) {
            Some(job) => helper::main(PathBuf::from(job).as_path()),
            None => 2,
        };
        std::process::exit(code);
    }
    let mut startup = Startup::default();
    let mut rest = Vec::new();
    let mut iter = args.drain(..);
    while let Some(arg) = iter.next() {
        if arg == RECEIPT_FLAG {
            startup.receipt = iter.next().map(PathBuf::from);
        } else if arg == ERROR_FLAG {
            startup.error = iter.next().map(|e| e.to_string_lossy().into_owned());
        } else {
            rest.push(arg.to_string_lossy().into_owned());
        }
    }
    if let Ok(mut slot) = STARTUP.lock() {
        *slot = startup;
    }
    rest
}

/// Once the window is up after an update: tells the waiting helper that
/// this version started. Returns the version it updated to, for a note.
pub fn acknowledge() -> Option<String> {
    let receipt = STARTUP.lock().ok()?.receipt.take()?;
    match job::acknowledge(&receipt) {
        Ok(()) => {
            log::info!("update: started as {VERSION}, told the helper");
            Some(VERSION.to_string())
        }
        Err(e) => {
            log::warn!("update: couldn't acknowledge {}: {e:#}", receipt.display());
            None
        }
    }
}

/// The message from a rolled back update, once.
pub fn startup_error() -> Option<String> {
    STARTUP.lock().ok()?.error.take()
}
