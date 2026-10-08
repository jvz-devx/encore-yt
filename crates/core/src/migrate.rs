//! The move from the app's former name (M23): on the first start as
//! Encore, the folders ytfast-gpui used (`~/.config/ytfast`,
//! `~/.cache/ytfast` and their macOS and Windows equivalents) become the new
//! ones, with the settings and sign-in cookies in them. Runs before anything
//! creates the new folders, so it never merges into one that exists.

use std::io;
use std::path::{Path, PathBuf};

/// The folder name before the rename.
pub const OLD_NAME: &str = "ytfast";

/// Moves `<base>/<old>` to `<base>/<new>` for each base directory whose new
/// folder doesn't exist yet, and returns the folders that moved. A rename
/// where it can; across filesystems a full copy first, and the old folder
/// goes only once the copy is complete, so a failure leaves it as it was.
pub fn move_folders(bases: &[&Path], old: &str, new: &str) -> io::Result<Vec<PathBuf>> {
    let mut moved = Vec::new();
    for base in bases {
        let (from, to) = (base.join(old), base.join(new));
        if to.exists() || !from.is_dir() || moved.contains(&to) {
            continue;
        }
        move_folder(&from, &to)?;
        moved.push(to);
    }
    Ok(moved)
}

fn move_folder(from: &Path, to: &Path) -> io::Result<()> {
    match std::fs::rename(from, to) {
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {}
        other => return other,
    }
    // Copied under a temporary name, so an interrupted copy is never taken
    // for the moved folder; the next start copies again.
    let partial = to.with_file_name(format!(
        "{}.moving",
        to.file_name().unwrap_or_default().to_string_lossy()
    ));
    if partial.exists() {
        std::fs::remove_dir_all(&partial)?;
    }
    if let Err(e) = copy_tree(from, &partial) {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(e);
    }
    std::fs::rename(&partial, to)?;
    // The new folder is complete; an old one left behind is only clutter.
    let _ = std::fs::remove_dir_all(from);
    Ok(())
}

/// Copies a folder with its files' permissions (the cookie files stay 0600,
/// the folders 0700).
fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir(to)?;
    std::fs::set_permissions(to, std::fs::metadata(from)?.permissions())?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// A chosen cookie file is saved as "<folder>/<file>" (`auth`), so its
/// folder name changes with the rename: "ytfast/cookies.txt" becomes
/// "encore-yt/cookies.txt". Other keys stay as they were.
pub fn rename_profile(settings: &Path, old: &str, new: &str) -> io::Result<bool> {
    let Ok(bytes) = std::fs::read(settings) else {
        return Ok(false);
    };
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(false);
    };
    let Some(profile) = value.get_mut("browser_profile") else {
        return Ok(false);
    };
    let Some(file) = profile
        .as_str()
        .and_then(|p| p.strip_prefix(&format!("{old}/")))
    else {
        return Ok(false);
    };
    *profile = format!("{new}/{file}").into();
    let bytes = serde_json::to_vec_pretty(&value).map_err(io::Error::other)?;
    crate::paths::write_atomic(settings, &bytes)?;
    Ok(true)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions report fixture failures"
)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("encore-migrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn moves_the_old_folders_with_their_files() {
        let root = temp("move");
        let (config, cache) = (root.join("config"), root.join("cache"));
        std::fs::create_dir_all(config.join("ytfast")).unwrap();
        std::fs::create_dir_all(cache.join("ytfast/pages")).unwrap();
        std::fs::write(config.join("ytfast/cookies.txt"), "cookie").unwrap();
        std::fs::write(
            config.join("ytfast/settings.json"),
            r#"{"browser_profile":"ytfast/cookies.txt","volume":40}"#,
        )
        .unwrap();
        std::fs::write(cache.join("ytfast/pages/home.json"), "{}").unwrap();

        let moved = move_folders(&[&config, &cache], "ytfast", "encore-yt").unwrap();
        assert_eq!(moved, [config.join("encore-yt"), cache.join("encore-yt")]);
        assert!(!config.join("ytfast").exists() && !cache.join("ytfast").exists());
        let cookie = std::fs::read_to_string(config.join("encore-yt/cookies.txt")).unwrap();
        assert_eq!(cookie, "cookie");
        assert!(cache.join("encore-yt/pages/home.json").is_file());

        let settings = config.join("encore-yt/settings.json");
        assert!(rename_profile(&settings, "ytfast", "encore-yt").unwrap());
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
        assert_eq!(value["browser_profile"], "encore-yt/cookies.txt");
        assert_eq!(value["volume"], 40);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn leaves_both_alone_when_the_new_folder_exists() {
        let root = temp("exists");
        std::fs::create_dir_all(root.join("ytfast")).unwrap();
        std::fs::create_dir_all(root.join("encore-yt")).unwrap();
        std::fs::write(root.join("ytfast/settings.json"), "{}").unwrap();

        let moved = move_folders(&[&root], "ytfast", "encore-yt").unwrap();
        assert!(moved.is_empty());
        assert!(root.join("ytfast/settings.json").is_file());
        assert!(!root.join("encore-yt/settings.json").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn copies_a_tree_with_its_permissions() {
        let root = temp("copy");
        std::fs::create_dir_all(root.join("ytfast/player")).unwrap();
        std::fs::write(root.join("ytfast/cookies.txt"), "cookie").unwrap();
        std::fs::write(root.join("ytfast/player/current"), "abcd").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let private = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(root.join("ytfast/cookies.txt"), private).unwrap();
        }

        copy_tree(&root.join("ytfast"), &root.join("copy")).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("copy/player/current")).unwrap(),
            "abcd"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.join("copy/cookies.txt"))
                .unwrap()
                .permissions();
            assert_eq!(mode.mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
