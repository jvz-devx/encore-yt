//! Which browsers the sign-in sheet's browser route can work with (M31).
//!
//! Encore reads a YouTube Music sign-in from a browser's cookie store (see
//! `auth`). Firefox-family browsers can be read on every OS and Chromium
//! ones on Linux and macOS, but on Windows Chromium (Edge, Chrome, Brave)
//! keeps its cookies under an app-bound key no other app can open. This
//! finds what is installed and the default browser, says which can be read,
//! and decides what the browser route does: wait as before, open a readable
//! browser instead of an unreadable default, or say at once that it can't
//! work.

use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// One installed browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Browser {
    /// "Firefox", "Microsoft Edge", "Google Chrome".
    pub name: String,
    pub readable: bool,
    /// Why it can't be read, in a short phrase; `None` when it can.
    pub why_not: Option<&'static str>,
    /// Its executable, where it was found (Windows).
    pub exe: Option<PathBuf>,
}

impl Browser {
    /// The name as people say it: "Edge", "Chrome".
    pub fn short(&self) -> &str {
        self.name
            .strip_prefix("Microsoft ")
            .or_else(|| self.name.strip_prefix("Google "))
            .unwrap_or(&self.name)
    }
}

/// What the system says: the installed browsers (by name, with the
/// executable where known) and the default one, if it knows.
#[derive(Clone, Debug)]
pub struct Facts {
    pub os: Os,
    pub installed: Vec<(String, Option<PathBuf>)>,
    pub default: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub os: Os,
    pub browsers: Vec<Browser>,
    /// The name of the default browser, when the OS said and it is known.
    pub default: Option<String>,
}

/// What the browser route does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Open YouTube Music (in `open`, else the system's default browser) and
    /// look through the readable browsers. `note` says why `open` differs
    /// from the default.
    Wait {
        open: Option<Browser>,
        note: Option<String>,
    },
    /// No installed browser can be read. `unreadable` names the installed
    /// ones that can't.
    Unreadable { unreadable: Vec<String> },
}

fn is_firefox_family(name: &str) -> bool {
    matches!(name, "Firefox" | "LibreWolf")
}

/// Why Encore can't read this browser's cookies on `os`; `None` if it can.
fn why_not(os: Os, name: &str) -> Option<&'static str> {
    if is_firefox_family(name) || os != Os::Windows {
        None
    } else {
        Some("its cookies are locked to the browser")
    }
}

/// Sorts the facts into readable and unreadable browsers. The default is
/// listed even when no executable was found for it.
pub fn classify(facts: Facts) -> Report {
    let mut browsers: Vec<Browser> = Vec::new();
    let names = facts
        .installed
        .into_iter()
        .chain(facts.default.clone().map(|d| (d, None)));
    for (name, exe) in names {
        if browsers.iter().any(|b| b.name == name) {
            continue;
        }
        let why_not = why_not(facts.os, &name);
        browsers.push(Browser {
            readable: why_not.is_none(),
            why_not,
            name,
            exe,
        });
    }
    Report {
        os: facts.os,
        browsers,
        default: facts.default,
    }
}

/// Decides what the browser route does for this report.
pub fn plan(report: &Report) -> Plan {
    let Some(first) = report.browsers.iter().find(|b| b.readable) else {
        // Elsewhere an unknown setup keeps the old behaviour: open the
        // system's browser and look.
        return if report.os == Os::Windows {
            Plan::Unreadable {
                unreadable: report.browsers.iter().map(|b| b.name.clone()).collect(),
            }
        } else {
            Plan::Wait {
                open: None,
                note: None,
            }
        };
    };
    let default = report
        .default
        .as_ref()
        .and_then(|d| report.browsers.iter().find(|b| &b.name == d));
    match default {
        Some(default) if !default.readable => Plan::Wait {
            note: Some(format!(
                "Opening {}, because {} can't be read.",
                first.short(),
                default.short()
            )),
            open: Some(first.clone()),
        },
        _ => Plan::Wait {
            open: None,
            note: None,
        },
    }
}

/// Looks at this system. A debug build honours `ENCORE_FAKE_BROWSERS`
/// (`windows-edge`, `windows-edge-firefox`) so the sheet can be checked
/// for another OS.
pub fn detect() -> Report {
    #[cfg(debug_assertions)]
    if let Some(report) = std::env::var("ENCORE_FAKE_BROWSERS")
        .ok()
        .and_then(|v| fake(&v))
    {
        return report;
    }
    let os = Os::current();
    let mut installed: Vec<(String, Option<PathBuf>)> = crate::auth::installed_browsers()
        .into_iter()
        .map(|n| (n.to_string(), None))
        .collect();
    if os == Os::Windows {
        for (name, exe) in windows_installed() {
            match installed.iter_mut().find(|(n, _)| *n == name) {
                Some(entry) => entry.1 = Some(exe),
                None => installed.push((name, Some(exe))),
            }
        }
    }
    let default = match os {
        Os::Linux => command_output("xdg-settings", &["get", "default-web-browser"])
            .and_then(|id| default_from_desktop_id(&id)),
        Os::Windows => command_output(
            "reg",
            &[
                "query",
                r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
                "/v",
                "ProgId",
            ],
        )
        .and_then(|out| default_from_reg_output(&out)),
        // The default needs private APIs; list what is installed only.
        Os::MacOs => None,
    };
    classify(Facts {
        os,
        installed,
        default,
    })
}

#[cfg(debug_assertions)]
fn fake(kind: &str) -> Option<Report> {
    let installed: &[&str] = match kind {
        "windows-edge" => &["Microsoft Edge"],
        "windows-edge-firefox" => &["Microsoft Edge", "Firefox"],
        _ => return None,
    };
    Some(classify(Facts {
        os: Os::Windows,
        installed: installed.iter().map(|n| (n.to_string(), None)).collect(),
        default: Some("Microsoft Edge".into()),
    }))
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    // A console program started from the GUI app would flash a console
    // window on Windows (`reg query`).
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let out = command.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// "firefox.desktop", "org.mozilla.firefox.desktop", "brave-browser.desktop".
pub fn default_from_desktop_id(id: &str) -> Option<String> {
    let id = id.trim().to_ascii_lowercase();
    let name = if id.contains("librewolf") {
        "LibreWolf"
    } else if id.contains("firefox") {
        "Firefox"
    } else if id.contains("edge") {
        "Microsoft Edge"
    } else if id.contains("brave") {
        "Brave"
    } else if id.contains("chromium") {
        "Chromium"
    } else if id.contains("chrome") {
        "Google Chrome"
    } else {
        return None;
    };
    Some(name.into())
}

/// The ProgId of `reg query ... /v ProgId`: "MSEdgeHTM", "ChromeHTML",
/// "BraveHTML", "FirefoxURL-308046B0AF4A39CB".
pub fn default_from_reg_output(output: &str) -> Option<String> {
    let prog_id = output
        .lines()
        .find(|l| l.trim_start().starts_with("ProgId"))?
        .split_whitespace()
        .last()?;
    let name = match prog_id {
        p if p.starts_with("MSEdge") => "Microsoft Edge",
        p if p.starts_with("Chrome") => "Google Chrome",
        p if p.starts_with("Brave") => "Brave",
        p if p.starts_with("Firefox") => "Firefox",
        p if p.starts_with("LibreWolf") => "LibreWolf",
        _ => return None,
    };
    Some(name.into())
}

/// The browsers installed in their usual places on Windows.
fn windows_installed() -> Vec<(String, PathBuf)> {
    const KNOWN: &[(&str, &str)] = &[
        ("Microsoft Edge", r"Microsoft\Edge\Application\msedge.exe"),
        ("Google Chrome", r"Google\Chrome\Application\chrome.exe"),
        (
            "Brave",
            r"BraveSoftware\Brave-Browser\Application\brave.exe",
        ),
        ("Firefox", r"Mozilla Firefox\firefox.exe"),
        ("LibreWolf", r"LibreWolf\librewolf.exe"),
    ];
    let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .collect();
    KNOWN
        .iter()
        .filter_map(|(name, rel)| {
            roots
                .iter()
                .map(|root| root.join(rel))
                .find(|p| p.is_file())
                .map(|exe| (name.to_string(), exe))
        })
        .collect()
}

/// Opens `url` in this browser. False when it couldn't be started, and the
/// caller opens the system's browser instead.
pub fn open_in(browser: &Browser, url: &str) -> bool {
    let started = if let Some(exe) = &browser.exe {
        Command::new(exe).arg(url).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open")
            .args(["-a", &browser.name, url])
            .spawn()
    } else {
        let program = match browser.name.as_str() {
            "Firefox" => "firefox",
            "LibreWolf" => "librewolf",
            "Google Chrome" => "google-chrome",
            "Chromium" => "chromium",
            _ => return false,
        };
        Command::new(program).arg(url).spawn()
    };
    started.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(os: Os, installed: &[&str], default: Option<&str>) -> Report {
        classify(Facts {
            os,
            installed: installed.iter().map(|n| (n.to_string(), None)).collect(),
            default: default.map(Into::into),
        })
    }

    fn readable(report: &Report) -> Vec<&str> {
        report
            .browsers
            .iter()
            .filter(|b| b.readable)
            .map(|b| b.name.as_str())
            .collect()
    }

    const WAIT: Plan = Plan::Wait {
        open: None,
        note: None,
    };

    #[test]
    fn windows_chromium_is_unreadable_with_a_reason() {
        let r = report(
            Os::Windows,
            &["Microsoft Edge", "Google Chrome", "Brave", "Firefox"],
            None,
        );
        assert_eq!(readable(&r), ["Firefox"]);
        let edge = &r.browsers[0];
        assert!(!edge.readable);
        assert!(edge.why_not.is_some());
        assert_eq!(edge.short(), "Edge");
    }

    #[test]
    fn linux_and_macos_chromium_stay_readable() {
        for os in [Os::Linux, Os::MacOs] {
            let r = report(os, &["Google Chrome", "Firefox", "Brave"], Some("Brave"));
            assert_eq!(readable(&r).len(), 3);
            assert_eq!(plan(&r), WAIT);
        }
    }

    #[test]
    fn windows_with_only_edge_cannot_work() {
        let r = report(Os::Windows, &["Microsoft Edge"], Some("Microsoft Edge"));
        assert_eq!(
            plan(&r),
            Plan::Unreadable {
                unreadable: vec!["Microsoft Edge".into()]
            }
        );
    }

    #[test]
    fn windows_with_nothing_found_cannot_work_either() {
        let r = report(Os::Windows, &[], None);
        assert!(matches!(plan(&r), Plan::Unreadable { .. }));
    }

    #[test]
    fn an_unreadable_default_opens_the_readable_browser() {
        let r = report(
            Os::Windows,
            &["Microsoft Edge", "Firefox"],
            Some("Microsoft Edge"),
        );
        let Plan::Wait { open, note } = plan(&r) else {
            panic!("should wait");
        };
        assert_eq!(open.unwrap().name, "Firefox");
        assert_eq!(
            note.as_deref(),
            Some("Opening Firefox, because Edge can't be read.")
        );
    }

    #[test]
    fn a_readable_or_unknown_default_is_left_to_the_system() {
        let r = report(Os::Windows, &["Microsoft Edge", "Firefox"], Some("Firefox"));
        assert_eq!(plan(&r), WAIT);
        let r = report(Os::Windows, &["Microsoft Edge", "Firefox"], None);
        assert_eq!(plan(&r), WAIT);
    }

    #[test]
    fn an_unlisted_unreadable_default_is_still_counted() {
        let r = report(Os::Windows, &["Firefox"], Some("Google Chrome"));
        assert_eq!(r.browsers.len(), 2);
        let Plan::Wait { open, .. } = plan(&r) else {
            panic!("should wait");
        };
        assert_eq!(open.unwrap().name, "Firefox");
    }

    #[test]
    fn nothing_found_on_linux_keeps_the_old_behaviour() {
        assert_eq!(plan(&report(Os::Linux, &[], None)), WAIT);
    }

    #[test]
    fn reads_the_default_browser() {
        assert_eq!(
            default_from_desktop_id("org.mozilla.firefox.desktop\n").as_deref(),
            Some("Firefox")
        );
        assert_eq!(
            default_from_desktop_id("brave-browser.desktop").as_deref(),
            Some("Brave")
        );
        assert_eq!(default_from_desktop_id("vivaldi-stable.desktop"), None);
        let out = "\r\nHKEY_CURRENT_USER\\...\\https\\UserChoice\r\n    ProgId    REG_SZ    MSEdgeHTM\r\n";
        assert_eq!(
            default_from_reg_output(out).as_deref(),
            Some("Microsoft Edge")
        );
        let out = "    ProgId    REG_SZ    FirefoxURL-308046B0AF4A39CB\r\n";
        assert_eq!(default_from_reg_output(out).as_deref(), Some("Firefox"));
    }
}
