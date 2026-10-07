//! The programs the backend runs: mpv plays, yt-dlp finds the streams and
//! deno solves YouTube's challenges for yt-dlp. The AppImage, the macOS app
//! and the Windows installer bundle all three next to the executable, so
//! those directories go first on PATH; the .deb and .rpm bundle yt-dlp and
//! deno and depend on the distribution's mpv. A plain notice names any tool
//! that is still missing, which only happens when an install is broken.

use std::path::{Path, PathBuf};

const TOOLS: &[&str] = &["mpv", "yt-dlp", "deno"];

/// Puts the bundled tools' directories in front of PATH (only those not on
/// it already, so a system install in /usr/bin keeps the user's order). On
/// macOS also Homebrew's, which apps started from Finder don't get. Call it
/// first thing, before any thread starts.
pub fn add_bundled_to_path() {
    let current: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let mut front: Vec<PathBuf> = bundled_dirs()
        .into_iter()
        .filter(|dir| dir.is_dir() && !current.contains(dir))
        .collect();
    let mut back: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        back.extend(
            ["/opt/homebrew/bin", "/usr/local/bin"]
                .map(PathBuf::from)
                .into_iter()
                .filter(|dir| dir.is_dir() && !current.contains(dir)),
        );
    }
    if front.is_empty() && back.is_empty() {
        return;
    }
    let mut all = std::mem::take(&mut front);
    all.extend(current);
    all.extend(back);
    if let Ok(path) = std::env::join_paths(all) {
        // SAFETY: called at the top of main, before any other thread runs.
        unsafe { std::env::set_var("PATH", path) };
    }
}

/// The executable's directory (mpv in the AppImage's `usr/bin` and the
/// macOS app's `Contents/MacOS`), `bin/` next to it (Windows, .deb, .rpm),
/// and on macOS the app bundle's `Contents/Resources/bin`.
fn bundled_dirs() -> Vec<PathBuf> {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return Vec::new();
    };
    let mut dirs = vec![dir.join("bin"), dir.clone()];
    if cfg!(target_os = "macos") {
        dirs.insert(0, dir.join("../Resources/bin"));
    }
    dirs.into_iter()
        .map(|dir| dir.canonicalize().unwrap_or(dir))
        .collect()
}

/// A sentence for the error strip when a tool isn't on PATH, with the one
/// way to get it back on this system: the installers bring every tool but
/// the .deb/.rpm's mpv, so mostly it's a fresh copy of the app.
pub fn missing_notice() -> Option<String> {
    let missing: Vec<&str> = TOOLS.iter().copied().filter(|t| !on_path(t)).collect();
    if missing.is_empty() {
        return None;
    }
    for tool in &missing {
        log::warn!("{tool} is not on PATH");
    }
    let names = match missing.as_slice() {
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => unreachable!(),
    };
    let them = if missing.len() == 1 { "it" } else { "them" };
    let how = if cfg!(target_os = "macos") || cfg!(windows) {
        format!("Reinstall Music to get {them} back")
    } else if std::env::var_os("APPIMAGE").is_some() {
        format!("Download the Music AppImage again to get {them} back")
    } else {
        format!("Install {them} from your distribution's packages")
    };
    Some(format!(
        "Music can't find {names}, so songs won't play. {how}, then open Music again."
    ))
}

fn on_path(tool: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let file = format!("{tool}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path).any(|dir| dir.join(&file).is_file())
}
