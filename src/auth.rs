//! Reads the YouTube session from the desktop's Chromium-family or Firefox
//! browser.
//!
//! The browser keeps its cookie store open, so the database is copied first.
//! Chromium values are encrypted with a key derived from the browser's "Safe
//! Storage" password in the Secret Service (see docs/integration.md); Firefox
//! stores them in the clear. Cookie values are secrets: nothing here logs them.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Context, Result, anyhow, bail};
use sha2::Digest;

/// One browser installation ytfast can read: its name, its config directory
/// under `~/.config`, and the `application` its Safe Storage password is
/// filed under.
struct Browser {
    name: &'static str,
    dir: &'static str,
    keyring: &'static str,
}

const BROWSERS: &[Browser] = &[
    Browser {
        name: "Brave Origin",
        dir: "BraveSoftware/Brave-Origin",
        keyring: "brave",
    },
    Browser {
        name: "Brave",
        dir: "BraveSoftware/Brave-Browser",
        keyring: "brave",
    },
    Browser {
        name: "Google Chrome",
        dir: "google-chrome",
        keyring: "chrome",
    },
    Browser {
        name: "Chromium",
        dir: "chromium",
        keyring: "chromium",
    },
];

/// One Firefox-family installation: its name and its profiles directory
/// (the one holding `profiles.ini`) under the home directory.
struct Gecko {
    name: &'static str,
    dir: &'static str,
}

const GECKOS: &[Gecko] = &[
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

#[derive(Clone)]
pub struct Cookie {
    pub host: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    /// Unix seconds; 0 for a session cookie.
    pub expires: i64,
}

/// A browser's YouTube and Google cookies.
#[derive(Clone)]
pub struct Session {
    /// "Google Chrome (Default)", for the account menu.
    pub source: String,
    /// The [`Profile::id`] it came from.
    pub profile: String,
    cookies: Vec<Cookie>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("source", &self.source)
            .field("cookies", &self.cookies.len())
            .finish()
    }
}

impl Session {
    /// The `Cookie` header for music.youtube.com: every youtube.com cookie,
    /// the most specific host winning a repeated name.
    pub fn header(&self) -> String {
        let mut chosen: Vec<&Cookie> = Vec::new();
        for cookie in self.cookies.iter().filter(|c| applies_to_music(&c.host)) {
            match chosen.iter_mut().find(|c| c.name == cookie.name) {
                Some(existing) if specificity(&cookie.host) > specificity(&existing.host) => {
                    *existing = cookie
                }
                Some(_) => {}
                None => chosen.push(cookie),
            }
        }
        chosen
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    /// The value InnerTube's SAPISIDHASH authorization is computed from.
    pub fn sapisid(&self) -> Option<&str> {
        ["SAPISID", "__Secure-3PAPISID"].iter().find_map(|name| {
            self.cookies
                .iter()
                .find(|c| c.name == *name && applies_to_music(&c.host))
                .map(|c| c.value.as_str())
        })
    }

    /// Writes the cookies as a Netscape cookie file (mode 0600) for yt-dlp.
    pub fn write_netscape(&self, path: &Path) -> Result<()> {
        let mut text = String::from("# Netscape HTTP Cookie File\n");
        for c in &self.cookies {
            let domain_flag = if c.host.starts_with('.') {
                "TRUE"
            } else {
                "FALSE"
            };
            let secure = if c.secure { "TRUE" } else { "FALSE" };
            text.push_str(&format!(
                "{}\t{domain_flag}\t{}\t{secure}\t{}\t{}\t{}\n",
                c.host, c.path, c.expires, c.name, c.value
            ));
        }
        let temporary = path.with_extension("tmp");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(text.as_bytes())?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    }
}

fn applies_to_music(host: &str) -> bool {
    host == "music.youtube.com"
        || host == ".music.youtube.com"
        || host == ".youtube.com"
        || host == "youtube.com"
}

fn specificity(host: &str) -> usize {
    host.trim_start_matches('.').len()
}

/// How a profile's cookie database is read.
enum Store {
    /// Encrypted with the browser's Safe Storage key.
    Chromium(&'static Browser),
    /// `moz_cookies`, in the clear.
    Firefox,
}

/// A profile's cookie database and when it last changed.
struct Candidate {
    store: Store,
    /// How settings name a profile: "google-chrome/Default",
    /// ".mozilla/firefox/abcd1234.default-release".
    id: String,
    /// "Google Chrome (Default)", "Firefox (default-release)".
    label: String,
    cookies: PathBuf,
    modified: SystemTime,
    /// Firefox's default profile for its installation, preferred on a tie.
    default: bool,
}

/// A browser profile signed in to YouTube.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub id: String,
    pub label: String,
}

/// Every Chromium-family and Firefox profile with a cookie store, most
/// recently used first.
fn candidates() -> Result<Vec<Candidate>> {
    let base = directories::BaseDirs::new().context("no home directory")?;
    let config = base.config_dir().to_path_buf();
    let mut candidates = Vec::new();
    for browser in BROWSERS {
        let Ok(entries) = std::fs::read_dir(config.join(browser.dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let cookies = entry.path().join("Cookies");
            let Ok(meta) = std::fs::metadata(&cookies) else {
                continue;
            };
            let profile = entry.file_name().to_string_lossy().into_owned();
            candidates.push(Candidate {
                store: Store::Chromium(browser),
                id: format!("{}/{profile}", browser.dir),
                label: format!("{} ({profile})", browser.name),
                cookies,
                modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                default: false,
            });
        }
    }
    for gecko in GECKOS {
        firefox_candidates(gecko, &base.home_dir().join(gecko.dir), &mut candidates);
    }
    candidates.sort_by_key(|c| std::cmp::Reverse((c.modified, c.default)));
    Ok(candidates)
}

/// The profiles `profiles.ini` lists under `root` that have a cookie store.
/// `installs.ini` (and `[Install…]` sections) name each installation's
/// default profile.
fn firefox_candidates(gecko: &Gecko, root: &Path, candidates: &mut Vec<Candidate>) {
    let Ok(profiles) = std::fs::read_to_string(root.join("profiles.ini")) else {
        return;
    };
    let profiles = ini(&profiles);
    let installs = std::fs::read_to_string(root.join("installs.ini")).unwrap_or_default();
    let defaults: Vec<&str> = ini(&installs)
        .iter()
        .chain(profiles.iter().filter(|(s, _)| s.starts_with("Install")))
        .filter_map(|(_, keys)| ini_value(keys, "Default"))
        .collect();
    for (section, keys) in &profiles {
        if !section.starts_with("Profile") {
            continue;
        }
        let Some(path) = ini_value(keys, "Path") else {
            continue;
        };
        let dir = if ini_value(keys, "IsRelative") == Some("0") {
            PathBuf::from(path)
        } else {
            root.join(path)
        };
        let cookies = dir.join("cookies.sqlite");
        let Ok(meta) = std::fs::metadata(&cookies) else {
            continue;
        };
        // Firefox writes to the WAL first; it changes when the profile is used.
        let modified = [meta.modified().ok(), mtime(&wal_of(&cookies))]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let name = ini_value(keys, "Name").unwrap_or(path);
        candidates.push(Candidate {
            store: Store::Firefox,
            id: format!("{}/{path}", gecko.dir),
            label: format!("{} ({name})", gecko.name),
            cookies,
            modified,
            default: defaults.contains(&path)
                || (defaults.is_empty() && ini_value(keys, "Default") == Some("1")),
        });
    }
}

type Section<'a> = (&'a str, Vec<(&'a str, &'a str)>);

/// The sections of an INI file and their `key=value` lines.
fn ini(text: &str) -> Vec<Section<'_>> {
    let mut sections: Vec<Section<'_>> = Vec::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name, Vec::new()));
        } else if let (Some((key, value)), Some(section)) =
            (line.split_once('='), sections.last_mut())
        {
            section.1.push((key.trim(), value.trim()));
        }
    }
    sections
}

fn ini_value<'a>(keys: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    keys.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

/// SQLite's write-ahead log beside `database`.
fn wal_of(database: &Path) -> PathBuf {
    let mut wal = database.as_os_str().to_owned();
    wal.push("-wal");
    PathBuf::from(wal)
}

/// The browser profiles signed in to YouTube, for choosing which account to use.
pub fn profiles(scratch: &Path) -> Vec<Profile> {
    candidates()
        .unwrap_or_default()
        .iter()
        .filter(|c| matches!(read_profile(c, scratch), Ok(Some(_))))
        .map(|c| Profile {
            id: c.id.clone(),
            label: c.label.clone(),
        })
        .collect()
}

/// Reads the YouTube sign-in from `preferred` (a [`Profile::id`]) when it
/// has one, else from the most recently used signed-in browser profile.
///
/// `scratch` is a private directory for the database copy.
pub fn load(scratch: &Path, preferred: Option<&str>) -> Result<Session> {
    let mut candidates = candidates()?;
    if candidates.is_empty() {
        bail!("No Chromium-family or Firefox browser profile was found");
    }
    if let Some(preferred) = preferred
        && let Some(i) = candidates.iter().position(|c| c.id == preferred)
    {
        let chosen = candidates.remove(i);
        candidates.insert(0, chosen);
    }
    let mut last_error = None;
    for candidate in &candidates {
        match read_profile(candidate, scratch) {
            Ok(Some(session)) => return Ok(session),
            Ok(None) => {}
            Err(error) => {
                log::warn!("could not read {}: {error:#}", candidate.label);
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow!("No browser profile is signed in to YouTube")))
}

fn read_profile(candidate: &Candidate, scratch: &Path) -> Result<Option<Session>> {
    let copy = scratch.join(format!("cookies-{}.sqlite", std::process::id()));
    std::fs::copy(&candidate.cookies, &copy).context("copying the cookie database")?;
    let wal = wal_of(&candidate.cookies);
    if wal.exists() {
        let _ = std::fs::copy(
            &wal,
            copy.with_file_name(format!("cookies-{}.sqlite-wal", std::process::id())),
        );
    }
    let result = match candidate.store {
        Store::Chromium(browser) => read_copy(candidate, browser, &copy),
        Store::Firefox => read_firefox_copy(candidate, &copy),
    };
    let _ = std::fs::remove_file(&copy);
    let _ = std::fs::remove_file(
        copy.with_file_name(format!("cookies-{}.sqlite-wal", std::process::id())),
    );
    result
}

type Row = (String, String, String, Vec<u8>, String, i64, bool);

/// Whether a cookie is the one SAPISIDHASH needs: the profile is signed in.
fn signs_in(host: &str, name: &str) -> bool {
    applies_to_music(host) && (name == "SAPISID" || name == "__Secure-3PAPISID")
}

fn read_copy(candidate: &Candidate, browser: &Browser, copy: &Path) -> Result<Option<Session>> {
    let db =
        rusqlite::Connection::open_with_flags(copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let version: i64 = db
        .query_row("SELECT value FROM meta WHERE key = 'version'", [], |row| {
            row.get::<_, String>(0)
        })
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut statement = db.prepare(
        "SELECT host_key, name, value, encrypted_value, path, expires_utc, is_secure FROM cookies \
         WHERE host_key LIKE '%youtube.com' OR host_key LIKE '%google.com'",
    )?;
    let rows: Vec<Row> = statement
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if !rows.iter().any(|r| signs_in(&r.0, &r.1)) {
        return Ok(None);
    }
    let key = derive_key(&keyring_password(browser.keyring)?);
    let mut cookies = Vec::with_capacity(rows.len());
    let mut failed = 0;
    for (host, name, value, encrypted, path, expires_utc, secure) in rows {
        let value = if encrypted.is_empty() {
            value
        } else {
            match decrypt(&encrypted, &key, &host, version) {
                Some(v) => v,
                None => {
                    failed += 1;
                    continue;
                }
            }
        };
        let expires = if expires_utc > 0 {
            (expires_utc / 1_000_000 - 11_644_473_600).max(0)
        } else {
            0
        };
        cookies.push(Cookie {
            host,
            name,
            value,
            path,
            secure,
            expires,
        });
    }
    if failed > 0
        && cookies
            .iter()
            .all(|c| c.name != "SAPISID" && c.name != "__Secure-3PAPISID")
    {
        bail!("{failed} cookies could not be decrypted; the browser's key did not match");
    }
    log::info!(
        "read {} cookies from {}, {failed} undecryptable",
        cookies.len(),
        candidate.label
    );
    Ok(Some(Session {
        source: candidate.label.clone(),
        profile: candidate.id.clone(),
        cookies,
    }))
}

type FirefoxRow = (String, String, String, String, i64, bool);

/// Reads `moz_cookies` from the default container (no container tab, not
/// partitioned).
fn read_firefox_copy(candidate: &Candidate, copy: &Path) -> Result<Option<Session>> {
    let db =
        rusqlite::Connection::open_with_flags(copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mut statement = db.prepare(
        "SELECT host, name, value, path, expiry, isSecure FROM moz_cookies \
         WHERE originAttributes = '' AND (host LIKE '%youtube.com' OR host LIKE '%google.com')",
    )?;
    let rows: Vec<FirefoxRow> = statement
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    if !rows.iter().any(|r| signs_in(&r.0, &r.1)) {
        return Ok(None);
    }
    let cookies: Vec<Cookie> = rows
        .into_iter()
        .map(|(host, name, value, path, expiry, secure)| Cookie {
            host,
            name,
            value,
            path,
            secure,
            // Seconds in older Firefox, milliseconds in newer (schema 17).
            expires: if expiry > 100_000_000_000 {
                expiry / 1000
            } else {
                expiry.max(0)
            },
        })
        .collect();
    log::info!("read {} cookies from {}", cookies.len(), candidate.label);
    Ok(Some(Session {
        source: candidate.label.clone(),
        profile: candidate.id.clone(),
        cookies,
    }))
}

fn keyring_password(application: &str) -> Result<Vec<u8>> {
    let output = Command::new("secret-tool")
        .args(["lookup", "application", application])
        .output()
        .context("running secret-tool (libsecret)")?;
    let mut password = output.stdout;
    while password.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        password.pop();
    }
    if password.is_empty() {
        // Chromium falls back to this fixed password without a keyring.
        return Ok(b"peanuts".to_vec());
    }
    Ok(password)
}

fn derive_key(password: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1, &mut key);
    key
}

fn decrypt(encrypted: &[u8], key: &[u8; 16], host: &str, version: i64) -> Option<String> {
    let body = match encrypted.get(..3) {
        Some(b"v10") | Some(b"v11") => &encrypted[3..],
        _ => return None,
    };
    let decryptor = cbc::Decryptor::<aes::Aes128>::new(key.into(), &[b' '; 16].into());
    let plain = decryptor.decrypt_padded_vec_mut::<Pkcs7>(body).ok()?;
    // Schema 24 prefixes the value with SHA-256 of its host.
    let plain = if version >= 24
        && plain.len() >= 32
        && plain[..32] == sha2::Sha256::digest(host.as_bytes())[..]
    {
        &plain[32..]
    } else {
        &plain[..]
    };
    String::from_utf8(plain.to_vec()).ok()
}
