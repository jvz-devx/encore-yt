//! Platform browser metadata and installation locations, without reading cookies.

use std::path::{Path, PathBuf};

/// One Chromium-family installation Encore can read: its name and its
/// profiles directory under the config directory (`~/.config` on Linux,
/// `~/Library/Application Support` on macOS). On Linux its Safe Storage
/// password is filed under `keyring` (the Secret Service's `application`)
/// or, on KDE, in KWallet (folder "<vendor> Keys", entry "<vendor> Safe
/// Storage", as yt-dlp reads them). On macOS it is the Keychain item
/// `keychain` with the account `vendor`.
pub(super) struct Browser {
    pub(super) name: &'static str,
    pub(super) dir: &'static str,
    #[cfg_attr(
        target_os = "macos",
        allow(dead_code, reason = "only Linux uses Secret Service keys")
    )]
    pub(super) keyring: &'static str,
    pub(super) vendor: &'static str,
    #[cfg_attr(
        not(target_os = "macos"),
        allow(dead_code, reason = "only macOS uses Keychain names")
    )]
    pub(super) keychain: &'static str,
}

#[cfg_attr(
    not(any(target_os = "linux", target_os = "macos")),
    allow(
        dead_code,
        reason = "browser import is implemented for Linux and macOS"
    )
)]
const fn browser(
    name: &'static str,
    dir: &'static str,
    keyring: &'static str,
    vendor: &'static str,
    keychain: &'static str,
) -> Browser {
    Browser {
        name,
        dir,
        keyring,
        vendor,
        keychain,
    }
}

#[cfg(target_os = "linux")]
pub(super) const BROWSERS: &[Browser] = &[
    browser(
        "Brave Origin",
        "BraveSoftware/Brave-Origin",
        "brave",
        "Brave",
        "",
    ),
    browser("Brave", "BraveSoftware/Brave-Browser", "brave", "Brave", ""),
    browser("Google Chrome", "google-chrome", "chrome", "Chrome", ""),
    browser("Chromium", "chromium", "chromium", "Chromium", ""),
];

/// Helium files its key as "Helium Storage Key" (imputnet/helium-macos,
/// `change-keychain-name.patch`); the others as yt-dlp reads them.
#[cfg(target_os = "macos")]
pub(super) const BROWSERS: &[Browser] = &[
    browser(
        "Helium",
        "net.imput.helium",
        "",
        "Helium",
        "Helium Storage Key",
    ),
    browser(
        "Google Chrome",
        "Google/Chrome",
        "",
        "Chrome",
        "Chrome Safe Storage",
    ),
    browser(
        "Brave",
        "BraveSoftware/Brave-Browser",
        "",
        "Brave",
        "Brave Safe Storage",
    ),
    browser(
        "Microsoft Edge",
        "Microsoft Edge",
        "",
        "Microsoft Edge",
        "Microsoft Edge Safe Storage",
    ),
    browser("Arc", "Arc/User Data", "", "Arc", "Arc Safe Storage"),
    browser(
        "Chromium",
        "Chromium",
        "",
        "Chromium",
        "Chromium Safe Storage",
    ),
];

/// Windows encrypts Chromium cookies with DPAPI and, since Chrome 127, an
/// app-bound key only the browser can open: not read here.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) const BROWSERS: &[Browser] = &[];

/// One Firefox-family installation: its name and its profiles directory
/// (the one holding `profiles.ini`), under the home directory on Linux and
/// under the config directory elsewhere (`~/Library/Application Support`,
/// `%APPDATA%`).
pub(super) struct Gecko {
    pub(super) name: &'static str,
    pub(super) dir: &'static str,
}

#[cfg(target_os = "linux")]
pub(super) const GECKOS: &[Gecko] = &[
    Gecko {
        name: "Firefox",
        dir: ".mozilla/firefox",
    },
    Gecko {
        name: "Firefox",
        dir: ".config/mozilla/firefox",
    },
    Gecko {
        name: "Firefox Flatpak",
        dir: ".var/app/org.mozilla.firefox/.mozilla/firefox",
    },
    Gecko {
        name: "LibreWolf",
        dir: ".librewolf",
    },
    Gecko {
        name: "LibreWolf Flatpak",
        dir: ".var/app/io.gitlab.librewolf-community/.librewolf",
    },
];

#[cfg(target_os = "macos")]
pub(super) const GECKOS: &[Gecko] = &[
    Gecko {
        name: "Firefox",
        dir: "Firefox",
    },
    Gecko {
        name: "LibreWolf",
        dir: "librewolf",
    },
];

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(super) const GECKOS: &[Gecko] = &[
    Gecko {
        name: "Firefox",
        dir: "Mozilla/Firefox",
    },
    Gecko {
        name: "LibreWolf",
        dir: "librewolf",
    },
];

/// Where a Firefox-family `dir` is: the home directory on Linux, the config
/// directory elsewhere.
pub(super) fn gecko_base(base: &directories::BaseDirs) -> &Path {
    if cfg!(target_os = "linux") {
        base.home_dir()
    } else {
        base.config_dir()
    }
}

/// Where the Chromium-family browsers keep their profiles. Normally the
/// config directory; inside a Flatpak sandbox that directory is the app's own
/// (`~/.var/app/<id>/config`), so look in the real `~/.config`, which the
/// manifest opens read-only for the known browsers.
pub(super) fn browser_config_dir(base: &directories::BaseDirs) -> PathBuf {
    if cfg!(target_os = "linux") && Path::new("/.flatpak-info").exists() {
        base.home_dir().join(".config")
    } else {
        base.config_dir().to_path_buf()
    }
}

/// The browsers Encore looks for on this system, for the sign-in sheet.
pub fn supported_browsers() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = GECKOS
        .iter()
        .map(|g| g.name.trim_end_matches(" Flatpak"))
        .chain(BROWSERS.iter().map(|b| b.name))
        .collect();
    let mut seen = Vec::new();
    names.retain(|n| {
        let new = !seen.contains(n);
        seen.push(*n);
        new
    });
    names
}

/// The supported browsers that have a profile directory on this system,
/// for the sign-in sheet's browser route (see `browsers`).
pub(crate) fn installed_browsers() -> Vec<&'static str> {
    let Some(base) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let config = browser_config_dir(&base);
    let mut names: Vec<&'static str> = BROWSERS
        .iter()
        .filter(|b| config.join(b.dir).is_dir())
        .map(|b| b.name)
        .chain(
            GECKOS
                .iter()
                .filter(|g| gecko_base(&base).join(g.dir).is_dir())
                .map(|g| g.name.trim_end_matches(" Flatpak")),
        )
        .collect();
    let mut seen = Vec::new();
    names.retain(|n| {
        let new = !seen.contains(n);
        seen.push(*n);
        new
    });
    names
}
