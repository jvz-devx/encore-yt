//! Where Encore keeps things.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use sha2::Digest;

/// Read optional JSON state. Missing files are normal on first launch;
/// unreadable or damaged files are reported without logging their contents.
pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("couldn't read {}: {error}", path.display());
            return None;
        }
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!(
                "ignoring invalid JSON in {} at line {}, column {} ({:?})",
                path.display(),
                error.line(),
                error.column(),
                error.classify()
            );
            None
        }
    }
}

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

/// Exclusive creation prevents following a pre-existing temporary symlink.
fn private_file() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
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

/// Atomically replace a file using a fresh private sibling. Concurrent writes
/// never share a temporary path, and an existing permissive destination is
/// replaced by a mode-0600 file on Unix.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..16 {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = path.with_extension(format!("tmp{}-{sequence}", std::process::id()));
        let mut file = match private_file().open(&temporary) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        let written = file.write_all(bytes);
        drop(file);
        let result = written.and_then(|()| std::fs::rename(&temporary, path));
        if result.is_err()
            && let Err(error) = std::fs::remove_file(&temporary)
        {
            log::warn!(
                "couldn't remove failed temporary write {}: {error}",
                temporary.display()
            );
        }
        return result;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "couldn't reserve a temporary state file",
    ))
}

/// The atomic writer for Tokio tasks. Filesystem work stays on the blocking pool.
pub async fn write_atomic_async(
    path: PathBuf,
    bytes: impl AsRef<[u8]> + Send + 'static,
) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || write_atomic(&path, bytes.as_ref()))
        .await
        .map_err(std::io::Error::other)?
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;

    #[test]
    fn optional_json_distinguishes_valid_state_from_missing_and_damaged_files() {
        let path = std::env::temp_dir().join(format!("encore-json-test-{}", std::process::id()));
        assert!(read_json::<Vec<u32>>(&path).is_none());
        std::fs::write(&path, "[1,2,3]").unwrap();
        assert_eq!(read_json::<Vec<u32>>(&path), Some(vec![1, 2, 3]));
        std::fs::write(&path, "not json").unwrap();
        assert!(read_json::<Vec<u32>>(&path).is_none());
        std::fs::write(&path, "{}").unwrap();
        assert!(read_json::<Vec<u32>>(&path).is_none());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(read_json::<Vec<u32>>(&path).is_none());
        std::fs::remove_dir(path).unwrap();
    }

    #[test]
    fn concurrent_writes_replace_whole_files_without_sharing_temporary_paths() {
        let dir = std::env::temp_dir().join(format!("encore-atomic-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, "old").unwrap();
        std::thread::scope(|scope| {
            for byte in 0..8 {
                let path = &path;
                scope.spawn(move || write_atomic(path, &vec![byte; 4096]).unwrap());
            }
        });
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 4096);
        assert!(bytes.iter().all(|b| *b == bytes[0]));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
