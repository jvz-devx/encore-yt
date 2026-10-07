//! How this copy was installed, which decides how it updates: from the
//! `APPIMAGE` variable the AppImage runtime sets, the setup program's
//! uninstaller next to the Windows executable, the `.app` bundle around
//! the macOS one, and `/usr/lib/ytfast-gpui` for the .deb and .rpm.

use std::path::{Path, PathBuf};

/// An installation the app replaces itself, or one it only tells about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// The AppImage file the app runs from.
    AppImage(PathBuf),
    /// `ytfast-gpui.exe` in the folder the setup program installed.
    Windows(PathBuf),
    /// The `.app` bundle.
    Mac(PathBuf),
    Manual(Manual),
}

/// Copies something else updates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manual {
    /// The .deb or .rpm.
    Package,
    /// The Windows portable zip.
    Portable,
    /// Built from source (`cargo run`, a copy somewhere).
    Source,
}

impl Install {
    /// This process's installation.
    pub fn detect() -> Self {
        let Ok(exe) = std::env::current_exe() else {
            return Self::Manual(Manual::Source);
        };
        if cfg!(target_os = "linux") {
            let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
            linux(&exe, appimage.as_deref())
        } else if cfg!(windows) {
            windows(&exe)
        } else if cfg!(target_os = "macos") {
            macos(&exe)
        } else {
            Self::Manual(Manual::Source)
        }
    }

    /// The release asset this installation updates from.
    pub fn asset(&self, version: &str) -> Option<String> {
        let arch = std::env::consts::ARCH;
        let suffix = match self {
            Self::AppImage(_) => format!("linux-{arch}.AppImage"),
            Self::Windows(_) => format!("windows-{arch}-setup.exe"),
            Self::Mac(_) => {
                let arch = if arch == "aarch64" { "arm64" } else { arch };
                format!("macos-{arch}.dmg")
            }
            Self::Manual(_) => return None,
        };
        Some(format!("ytfast-gpui-{version}-{suffix}"))
    }

    /// One line on how to update a copy the app doesn't replace.
    pub fn advice(&self) -> Option<&'static str> {
        match self {
            Self::Manual(Manual::Package) => Some("Update with your package manager"),
            Self::Manual(Manual::Portable) => {
                Some("Download the new portable zip from the release page")
            }
            Self::Manual(Manual::Source) => Some("Pull and build the new version"),
            _ => None,
        }
    }
}

fn linux(exe: &Path, appimage: Option<&Path>) -> Install {
    if let Some(file) = appimage.filter(|f| f.is_absolute() && f.is_file()) {
        return Install::AppImage(file.to_path_buf());
    }
    if exe.starts_with("/usr/lib/ytfast-gpui") {
        return Install::Manual(Manual::Package);
    }
    Install::Manual(Manual::Source)
}

fn windows(exe: &Path) -> Install {
    let dir = exe.parent().unwrap_or(exe);
    if dir.join("unins000.exe").is_file() {
        Install::Windows(exe.to_path_buf())
    } else if dir.join("THIRD-PARTY.txt").is_file() {
        Install::Manual(Manual::Portable)
    } else {
        Install::Manual(Manual::Source)
    }
}

/// `…/ytfast.app/Contents/MacOS/ytfast-gpui`.
fn macos(exe: &Path) -> Install {
    let bundle = exe
        .parent()
        .filter(|d| d.ends_with("Contents/MacOS"))
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|b| b.extension().is_some_and(|e| e == "app"));
    match bundle {
        Some(bundle) => Install::Mac(bundle.to_path_buf()),
        None => Install::Manual(Manual::Source),
    }
}
