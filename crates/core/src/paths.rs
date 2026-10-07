//! Where Encore keeps things.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::Digest;

/// The folder name under the config, cache and runtime directories, and
/// the command's name.
pub const NAME: &str = "encore-yt";

/// The one line saying what moved from the former name's folders, for the
/// log (logging starts after the folders are known).
static MIGRATED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// What the first start after the rename moved, if anything.
pub fn migration_note() -> Option<&'static str> {
    MIGRATED.get().map(String::as_str)
}

#[derive(Clone, Debug)]
pub struct Paths {
    /// `~/.config/encore-yt`: themes.
    pub config: PathBuf,
    /// `~/.cache/encore-yt`: pages and covers (personal data, 0700).
    pub cache: PathBuf,
    /// `$XDG_RUNTIME_DIR/encore-yt` (0700): resolved streams and the
    /// single-instance socket. Gone at logout.
    pub runtime: PathBuf,
}

impl Paths {
    pub fn new() -> Result<Self> {
        migrate_old_folders();
        let dirs = directories::ProjectDirs::from("", "", NAME).context("no home directory")?;
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(NAME);
        let paths = Self {
            config: dirs.config_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
            runtime,
        };
        for dir in [
            &paths.config,
            &paths.cache,
            &paths.cache.join("pages"),
            &paths.cache.join("covers"),
            &paths.runtime,
        ] {
            private_dir(dir)?;
        }
        Ok(paths)
    }

    pub fn page_file(&self, key: &str) -> PathBuf {
        self.cache.join("pages").join(format!("{}.json", hash(key)))
    }

    pub fn cover_file(&self, uri: &str) -> PathBuf {
        self.cache.join("covers").join(hash(uri))
    }

    pub fn searches_file(&self) -> PathBuf {
        self.cache.join("searches.json")
    }
}

/// Moves `~/.config/encore-yt` and `~/.cache/encore-yt` (and the macOS and
/// Windows equivalents) to the new name on the first start after the
/// rename, before anything makes the new folders. A failure leaves the old
/// folders as they were and is noted; the app then starts fresh.
fn migrate_old_folders() {
    let Some(base) = directories::BaseDirs::new() else {
        return;
    };
    let (config, cache) = (base.config_dir(), base.cache_dir());
    let note = match crate::migrate::move_folders(&[config, cache], crate::migrate::OLD_NAME, NAME)
    {
        Ok(moved) if moved.is_empty() => return,
        Ok(moved) => {
            let settings = directories::ProjectDirs::from("", "", NAME)
                .map(|dirs| dirs.config_dir().join("settings.json"));
            if let Some(settings) = settings
                && let Err(e) =
                    crate::migrate::rename_profile(&settings, crate::migrate::OLD_NAME, NAME)
            {
                log::warn!("settings: {e}");
            }
            let moved: Vec<_> = moved.iter().map(|p| p.display().to_string()).collect();
            format!(
                "moved the {} folders to {}",
                crate::migrate::OLD_NAME,
                moved.join(" and ")
            )
        }
        Err(e) => format!(
            "couldn't move the {} folders: {e}",
            crate::migrate::OLD_NAME
        ),
    };
    let _ = MIGRATED.set(note);
}

fn private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))
}

/// Options that create (or truncate) a file only this user can read: mode
/// 0600 on Unix. Windows keeps the profile's own access rules.
pub fn private_file() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
}

pub fn hash(text: &str) -> String {
    sha2::Sha256::digest(text.as_bytes())[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Writes through a temporary file so a crash never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(temporary, path)
}
