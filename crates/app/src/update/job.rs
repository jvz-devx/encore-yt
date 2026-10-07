//! The staging folder and the job the app hands to the helper.
//!
//! An update is downloaded into `.encore-yt-update-<16 hex digits>` next
//! to what it replaces (so the AppImage and the bundle swap by renaming,
//! on one file system), mode 0700. It holds the payload, `job.json`, the
//! helper's copy of the app and its log, `ready` (the helper is watching),
//! `started` (the new version is up), `previous` (the old version, kept for
//! rollback) and `result.txt`.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const JOB: &str = "job.json";
pub const READY: &str = "ready";
pub const STARTED: &str = "started";
pub const RESULT: &str = "result.txt";
pub const HELPER_LOG: &str = "helper.log";
pub const PREVIOUS: &str = "previous";
/// macOS: the new bundle, copied out of the disk image and checked.
pub const NEW_APP: &str = "new.app";
const PREFIX: &str = ".encore-yt-update-";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    AppImage,
    Windows,
    Mac,
}

/// What the helper installs, and where.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub kind: Kind,
    /// The AppImage file, the installed executable, or the bundle.
    pub target: PathBuf,
    /// The staging folder.
    pub dir: PathBuf,
    /// The verified download: the AppImage, the setup program, the disk
    /// image (whose bundle the app copied out to [`NEW_APP`]).
    pub payload: PathBuf,
    pub sha256: String,
    pub version: String,
}

impl Job {
    pub fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).context("Can't read the update job")?;
        let job: Self = serde_json::from_slice(&bytes).context("The update job is damaged")?;
        job.check_layout(path)?;
        Ok(job)
    }

    pub fn write(&self) -> Result<PathBuf> {
        let path = self.file(JOB);
        let bytes = serde_json::to_vec(self)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut f| f.write_all(&bytes))
            .context("Can't write the update job")?;
        Ok(path)
    }

    /// The job sits in a staging folder next to its target, and the payload
    /// inside that folder, so a job file can't point anywhere else.
    fn check_layout(&self, path: &Path) -> Result<()> {
        ensure!(
            path == self.file(JOB),
            "The update job is in the wrong place"
        );
        ensure!(
            is_staging(&self.dir),
            "The update job is in the wrong place"
        );
        ensure!(
            self.dir.parent() == self.target.parent(),
            "The update job belongs to another installation"
        );
        ensure!(
            self.payload.parent() == Some(self.dir.as_path()),
            "The update job names a file outside its folder"
        );
        Ok(())
    }
}

/// Makes a new staging folder in `parent`, after removing the ones earlier
/// updates left (each kept its `previous` for rollback until now).
pub fn new_staging(parent: &Path) -> Result<PathBuf> {
    remove_old(parent);
    let stamp = format!("{}-{:?}", std::process::id(), std::time::SystemTime::now());
    let hex = hex(&Sha256::digest(stamp.as_bytes()));
    let dir = parent.join(format!("{PREFIX}{}", &hex[..16]));
    private_dir()
        .create(&dir)
        .with_context(|| format!("Encore can't write to {}", parent.display()))?;
    Ok(dir)
}

/// A folder only this user can open (on Unix).
fn private_dir() -> fs::DirBuilder {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    }
    #[cfg(not(unix))]
    fs::DirBuilder::new()
}

fn remove_old(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_staging(&path)
            && let Err(e) = fs::remove_dir_all(&path)
        {
            log::warn!("update: couldn't remove {}: {e}", path.display());
        }
    }
}

fn is_staging(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix(PREFIX))
        .is_some_and(|hex| hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of a file, in lowercase hex.
pub fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hex(&hasher.finalize()))
}

/// The new version, started with a receipt: says so by writing `started`.
pub fn acknowledge(path: &Path) -> Result<()> {
    let job = Job::read(path)?;
    ensure!(
        job.version == super::VERSION,
        "the receipt is for {}, this is {}",
        job.version,
        super::VERSION
    );
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(job.file(STARTED))
        .and_then(|mut f| f.write_all(super::VERSION.as_bytes()))
        .context("already acknowledged")?;
    Ok(())
}
