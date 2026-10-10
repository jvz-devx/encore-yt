//! Reads the YouTube session from the desktop's Chromium-family or Firefox
//! browser, or from cookie files exported elsewhere
//! (`~/.config/encore-yt/*cookies*.txt`), and saves imported or pasted
//! cookies as such a file.
//!
//! The browser keeps its cookie store open, so the database is copied first.
//! Chromium values are encrypted with a key derived from the browser's "Safe
//! Storage" password, kept in the Secret Service or, on KDE, in KWallet (see
//! docs/integration.md), and on macOS in the Keychain; Firefox stores them
//! in the clear (read on every OS). Chromium on Windows (DPAPI and an
//! app-bound key) is not read. Cookie values and that password are secrets:
//! nothing here logs them.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow, bail};

mod browser;
mod cookies;
mod crypto;

pub use cookies::{Cookie, Session};
use cookies::{applies_to_music, parse_cookie_header, parse_netscape};

#[cfg(test)]
pub(crate) use cookies::parse_netscape as test_parse_netscape;

#[cfg(not(target_os = "macos"))]
use crypto::safe_storage_password;
use crypto::{Keys, decrypt, derive_key};
#[cfg(target_os = "macos")]
use crypto::{MAC_ITERATIONS, keychain_password};

pub(crate) use browser::installed_browsers;
pub use browser::supported_browsers;
use browser::{BROWSERS, Browser, GECKOS, Gecko, browser_config_dir, gecko_base};

/// How a profile's cookie database is read.
enum Store {
    /// Encrypted with the browser's Safe Storage key.
    Chromium(&'static Browser),
    /// `moz_cookies`, in the clear.
    Firefox,
    /// A Netscape cookie file the person put in Encore's config directory.
    CookieFile,
}

/// Where cookie files go, under the config directory: every
/// `*cookies*.txt` there ("cookies.txt", "browser-cookies.txt").
const COOKIE_DIR: &str = crate::paths::NAME;

fn is_cookie_file(name: &str) -> bool {
    name.ends_with(".txt") && name.contains("cookies")
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
    let config = browser_config_dir(&base);
    let mut candidates = Vec::new();
    for browser in BROWSERS {
        let Ok(entries) = std::fs::read_dir(config.join(browser.dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            // Newer Chromium keeps it under Network/ (as on Windows).
            let Some((cookies, meta)) = ["Cookies", "Network/Cookies"].iter().find_map(|name| {
                let path = entry.path().join(name);
                std::fs::metadata(&path).ok().map(|meta| (path, meta))
            }) else {
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
        firefox_candidates(gecko, &gecko_base(&base).join(gecko.dir), &mut candidates);
    }
    cookie_file_candidates(&config.join(COOKIE_DIR), &mut candidates);
    candidates.sort_by_key(|c| std::cmp::Reverse((c.modified, c.default)));
    Ok(candidates)
}

/// Every `*cookies*.txt` file in `dir`, as "Cookie file (browser-cookies.txt)"
/// with the id "encore-yt/browser-cookies.txt".
fn cookie_file_candidates(dir: &Path, candidates: &mut Vec<Candidate>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if !is_cookie_file(&name) || !path.is_file() {
            continue;
        }
        candidates.push(Candidate {
            store: Store::CookieFile,
            id: format!("{COOKIE_DIR}/{name}"),
            label: format!("Cookie file ({name})"),
            modified: mtime(&path).unwrap_or(SystemTime::UNIX_EPOCH),
            cookies: path,
            default: false,
        });
    }
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

/// What a look through the installed browsers found, for "Sign in with
/// your browser": every browser profile checked and the ones signed in.
/// Cookie files are left out.
#[derive(Clone, Debug, Default)]
pub struct BrowserScan {
    /// "Firefox (default-release)", "Google Chrome (Default)".
    pub checked: Vec<String>,
    pub signed_in: Vec<Profile>,
}

/// Looks through the browser profiles (not cookie files) for a YouTube
/// sign-in. Cheap enough to repeat every few seconds: a profile's Safe
/// Storage password is asked for only once it holds a sign-in cookie.
pub fn scan_browsers(scratch: &Path) -> BrowserScan {
    let mut scan = BrowserScan::default();
    for candidate in candidates().unwrap_or_default() {
        if matches!(candidate.store, Store::CookieFile) {
            continue;
        }
        if matches!(read_profile(&candidate, scratch), Ok(Some(_))) {
            scan.signed_in.push(Profile {
                id: candidate.id.clone(),
                label: candidate.label.clone(),
            });
        }
        scan.checked.push(candidate.label);
    }
    scan
}

/// Where imported and pasted cookies go (the directory cookie files are
/// read from): `~/.config/encore-yt`, `~/Library/Application Support/encore-yt`,
/// `%APPDATA%\encore-yt`.
fn cookie_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().context("no home directory")?;
    Ok(base.config_dir().join(COOKIE_DIR))
}

/// The file "Import a cookies file" saves to, and the one "Paste cookies"
/// saves to. Both are ordinary cookie files afterwards.
const IMPORTED: &str = "imported-cookies.txt";
const PASTED: &str = "pasted-cookies.txt";

/// Copies the YouTube and Google lines of a Netscape cookie file (from a
/// browser extension or yt-dlp) into the config directory, private to this
/// user (0600 on Unix; `%APPDATA%` is the user's own on Windows), and
/// returns it as a profile to sign in with. The source may be readable by
/// others; the copy is not.
pub fn import_cookie_file(source: &Path) -> Result<Profile> {
    let text = std::fs::read(source)
        .map_err(|error| anyhow!("Couldn't open that file ({})", error.kind()))?;
    let text = String::from_utf8_lossy(&text);
    let cookies = parse_netscape(&text);
    if cookies.is_empty() {
        bail!(
            "That file has no YouTube cookies. Export a cookies.txt from a browser signed in to YouTube Music."
        );
    }
    store(IMPORTED, cookies)
}

/// Saves a pasted `Cookie` header (or a pasted cookies.txt) as a cookie
/// file in the config directory, like [`import_cookie_file`].
pub fn store_cookie_header(text: &str) -> Result<Profile> {
    let cookies = if text.contains('\t') {
        parse_netscape(text)
    } else {
        parse_cookie_header(text)
    };
    // Counts only, never the text.
    log::info!(
        "pasted {} characters, {} cookies",
        text.chars().count(),
        cookies.len()
    );
    if cookies.is_empty() {
        bail!(
            "That doesn't look like a Cookie header. Copy the value of the Cookie request header, then paste it again."
        );
    }
    store(PASTED, cookies)
}

/// Writes `cookies` as `name` in the cookie directory once they hold a
/// sign-in.
fn store(name: &str, cookies: Vec<Cookie>) -> Result<Profile> {
    if !cookies.iter().any(|c| signs_in(&c.host, &c.name)) {
        bail!(
            "Those cookies aren't signed in to YouTube. Sign in to YouTube Music in the browser, then copy them again."
        );
    }
    let dir = cookie_dir()?;
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
        .create(&dir)
        .map_err(|error| anyhow!("Couldn't create the settings folder ({})", error.kind()))?;
    let label = format!("Cookie file ({name})");
    let session = Session {
        source: label.clone(),
        profile: format!("{COOKIE_DIR}/{name}"),
        cookies,
        persist: None,
    };
    session
        .write_netscape(&dir.join(name))
        .map_err(|error| anyhow!("Couldn't save the cookies ({error})"))?;
    log::info!("saved {} cookies as {label}", session.cookies.len());
    Ok(Profile {
        id: session.profile,
        label,
    })
}

/// Reads the YouTube sign-in from `preferred` (a [`Profile::id`]) when it
/// has one, else from the most recently used signed-in browser profile.
///
/// `scratch` is a private directory for the database copy.
pub fn load(scratch: &Path, preferred: Option<&str>) -> Result<Session> {
    let mut candidates = candidates()?;
    if candidates.is_empty() {
        bail!("No Chromium-family or Firefox browser profile or cookie file was found");
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
    with_copy(candidate, scratch, |copy| match candidate.store {
        Store::Chromium(browser) => read_copy(candidate, browser, copy),
        Store::Firefox => read_firefox_copy(candidate, copy),
        Store::CookieFile => read_cookie_file(candidate, copy),
    })
}

/// Runs `read` on a private copy of the profile's cookie database (with its
/// write-ahead log), removed afterwards. A cookie file is read in place.
fn with_copy<T>(
    candidate: &Candidate,
    scratch: &Path,
    read: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    if matches!(candidate.store, Store::CookieFile) {
        return read(&candidate.cookies);
    }
    // Unique per read: a sign-in scan can run while the app connects.
    static READS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let read_no = READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let copy = scratch.join(format!("cookies-{}-{read_no}.sqlite", std::process::id()));
    let copy_wal = wal_of(&copy);
    let mut copies = Copies::default();
    copies
        .copy(&candidate.cookies, &copy)
        .context("copying the cookie database")?;
    let wal = wal_of(&candidate.cookies);
    if let Err(error) = copies.copy(&wal, &copy_wal)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error).context("copying the cookie write-ahead log");
    }
    read(&copy)
}

#[derive(Default)]
struct Copies(Vec<PathBuf>);

impl Copies {
    fn copy(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        let mut source = std::fs::File::open(from)?;
        let mut copy = crate::paths::create_private(to)?;
        // Register only files we created, before a potentially failing copy.
        self.0.push(to.to_owned());
        std::io::copy(&mut source, &mut copy)?;
        Ok(())
    }
}

impl Drop for Copies {
    fn drop(&mut self) {
        for path in &self.0 {
            if let Err(error) = std::fs::remove_file(path)
                && error.kind() != std::io::ErrorKind::NotFound
            {
                log::warn!("couldn't remove private browser snapshot: {error}");
            }
        }
    }
}

/// What Encore can read from one browser profile, to diagnose sign-in.
/// Counts only: no cookie value or password leaves this module.
#[derive(Clone, Debug)]
pub struct Inspection {
    /// "Google Chrome (Default)".
    pub label: String,
    /// The [`Profile::id`].
    pub id: String,
    /// youtube.com and google.com cookies read (decrypted where needed).
    pub cookies: usize,
    /// Encrypted values no key opened.
    pub undecryptable: usize,
    /// `v12` values (the desktop portal's key), which Encore doesn't read.
    pub portal: usize,
    /// Whether the cookies read include the one sign-in needs (SAPISID).
    pub signed_in: bool,
    /// Where the Safe Storage password came from ("KWallet"), if needed.
    pub password: Option<&'static str>,
    /// Why the profile couldn't be read at all.
    pub error: Option<String>,
}

/// Reads every profile Encore would consider, signed in or not, and
/// reports what it found (`examples/sign_in.rs`). Unlike [`load`], this
/// asks for a Chromium profile's Safe Storage password even when the
/// profile isn't signed in.
pub fn inspect(scratch: &Path) -> Vec<Inspection> {
    candidates()
        .unwrap_or_default()
        .iter()
        .map(|candidate| {
            let mut inspection = Inspection {
                label: candidate.label.clone(),
                id: candidate.id.clone(),
                cookies: 0,
                undecryptable: 0,
                portal: 0,
                signed_in: false,
                password: None,
                error: None,
            };
            let read = with_copy(candidate, scratch, |copy| {
                let cookies = match candidate.store {
                    Store::Chromium(browser) => {
                        let (version, rows) = chromium_rows(copy)?;
                        let decrypted = decrypt_rows(browser, version, rows)?;
                        inspection.undecryptable = decrypted.failed;
                        inspection.portal = decrypted.portal;
                        inspection.password = decrypted.password;
                        decrypted.cookies
                    }
                    Store::Firefox => firefox_cookies(firefox_rows(copy)?),
                    Store::CookieFile => cookie_file_cookies(copy)?,
                };
                inspection.cookies = cookies.len();
                inspection.signed_in = cookies.iter().any(|c| signs_in(&c.host, &c.name));
                Ok(())
            });
            if let Err(error) = read {
                inspection.error = Some(format!("{error:#}"));
            }
            inspection
        })
        .collect()
}

type Row = (String, String, String, Vec<u8>, String, i64, bool);

/// Whether a cookie is the one SAPISIDHASH needs: the profile is signed in.
fn signs_in(host: &str, name: &str) -> bool {
    applies_to_music(host) && (name == "SAPISID" || name == "__Secure-3PAPISID")
}

/// The youtube.com and google.com rows of a copied Chromium cookie database,
/// and its schema version.
fn chromium_rows(copy: &Path) -> Result<(i64, Vec<Row>)> {
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
    Ok((version, rows))
}

/// What decrypting a Chromium profile's rows gave.
struct Decrypted {
    cookies: Vec<Cookie>,
    /// Values no key opened.
    failed: usize,
    /// `v12` values, encrypted with the desktop portal's key, which only the
    /// browser itself can ask for.
    portal: usize,
    /// Where the `v11` password came from, if any value needed it.
    password: Option<&'static str>,
}

/// The keys for a profile's rows on Linux: `v10` with Chromium's fixed
/// password, `v11` with the Safe Storage one (asked for only when a row
/// needs it), and where that came from.
#[cfg(not(target_os = "macos"))]
fn keys_for(browser: &Browser, rows: &[Row]) -> Result<(Keys, Option<&'static str>)> {
    let needs_password = rows.iter().any(|r| r.3.starts_with(b"v11"));
    let (v11, password) = if needs_password {
        match safe_storage_password(browser)? {
            Some((password, source)) => (Some(derive_key(&password, 1)), Some(source)),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    let keys = Keys {
        v10: Some(derive_key(b"peanuts", 1)),
        v11,
    };
    Ok((keys, password))
}

/// The keys for a profile's rows on macOS: `v10` with the Keychain's Safe
/// Storage password (macOS asks the person to allow it once).
#[cfg(target_os = "macos")]
fn keys_for(browser: &Browser, rows: &[Row]) -> Result<(Keys, Option<&'static str>)> {
    let needs_password = rows.iter().any(|r| r.3.starts_with(b"v10"));
    let (v10, password) = match needs_password.then(|| keychain_password(browser)).flatten() {
        Some(password) => (
            Some(derive_key(&password, MAC_ITERATIONS)),
            Some("the Keychain"),
        ),
        None => (None, None),
    };
    Ok((Keys { v10, v11: None }, password))
}

/// Where the Safe Storage password should have been, for the error.
const PASSWORD_HOME: &str = if cfg!(target_os = "macos") {
    "isn't in the Keychain, or access to it was denied"
} else {
    "is in neither the Secret Service nor KWallet"
};

fn decrypt_rows(browser: &Browser, version: i64, rows: Vec<Row>) -> Result<Decrypted> {
    let (keys, password) = keys_for(browser, &rows)?;
    let mut decrypted = Decrypted {
        cookies: Vec::with_capacity(rows.len()),
        failed: 0,
        portal: 0,
        password,
    };
    for (host, name, value, encrypted, path, expires_utc, secure) in rows {
        let value = if encrypted.is_empty() {
            value
        } else if encrypted.starts_with(b"v12") {
            decrypted.portal += 1;
            continue;
        } else {
            match decrypt(&encrypted, &keys, &host, version) {
                Some(v) => v,
                None => {
                    decrypted.failed += 1;
                    continue;
                }
            }
        };
        let expires = if expires_utc > 0 {
            (expires_utc / 1_000_000 - 11_644_473_600).max(0)
        } else {
            0
        };
        decrypted.cookies.push(Cookie {
            host,
            name,
            value,
            path,
            secure,
            expires,
        });
    }
    Ok(decrypted)
}

fn read_copy(candidate: &Candidate, browser: &Browser, copy: &Path) -> Result<Option<Session>> {
    let (version, rows) = chromium_rows(copy)?;
    if !rows.iter().any(|r| signs_in(&r.0, &r.1)) {
        return Ok(None);
    }
    let Decrypted {
        cookies,
        failed,
        portal,
        password,
    } = decrypt_rows(browser, version, rows)?;
    if !cookies.iter().any(|c| signs_in(&c.host, &c.name)) {
        if portal > 0 {
            bail!(
                "{} keeps its sign-in encrypted with the desktop portal's key, which Encore can't read",
                candidate.label
            );
        }
        bail!(
            "{failed} cookies of {} could not be decrypted; its Safe Storage password {}",
            candidate.label,
            if password.is_some() {
                "did not match"
            } else {
                PASSWORD_HOME
            }
        );
    }
    log::info!(
        "read {} cookies from {} (password from {}), {failed} undecryptable, {portal} portal-encrypted",
        cookies.len(),
        candidate.label,
        password.unwrap_or("nowhere")
    );
    Ok(Some(Session {
        source: candidate.label.clone(),
        profile: candidate.id.clone(),
        cookies,
        persist: None,
    }))
}

type FirefoxRow = (String, String, String, String, i64, bool);

/// `moz_cookies` rows of the default container (no container tab, not
/// partitioned).
fn firefox_rows(copy: &Path) -> Result<Vec<FirefoxRow>> {
    let db =
        rusqlite::Connection::open_with_flags(copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let mut statement = db.prepare(
        "SELECT host, name, value, path, expiry, isSecure FROM moz_cookies \
         WHERE originAttributes = '' AND (host LIKE '%youtube.com' OR host LIKE '%google.com')",
    )?;
    let rows = statement
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
    Ok(rows)
}

fn firefox_cookies(rows: Vec<FirefoxRow>) -> Vec<Cookie> {
    rows.into_iter()
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
        .collect()
}

fn read_firefox_copy(candidate: &Candidate, copy: &Path) -> Result<Option<Session>> {
    let rows = firefox_rows(copy)?;
    if !rows.iter().any(|r| signs_in(&r.0, &r.1)) {
        return Ok(None);
    }
    let cookies = firefox_cookies(rows);
    log::info!("read {} cookies from {}", cookies.len(), candidate.label);
    Ok(Some(Session {
        source: candidate.label.clone(),
        profile: candidate.id.clone(),
        cookies,
        persist: None,
    }))
}

fn read_cookie_file(candidate: &Candidate, path: &Path) -> Result<Option<Session>> {
    let cookies = cookie_file_cookies(path)?;
    if !cookies.iter().any(|c| signs_in(&c.host, &c.name)) {
        return Ok(None);
    }
    log::info!("read {} cookies from {}", cookies.len(), candidate.label);
    // Renew only the slots Encore created, never a browser database or a
    // hand-managed export (including symlinks to one).
    let owned = path
        .file_name()
        .is_some_and(|name| name == IMPORTED || name == PASTED)
        && std::fs::symlink_metadata(path)?.file_type().is_file();
    Ok(Some(Session {
        source: candidate.label.clone(),
        profile: candidate.id.clone(),
        cookies,
        persist: owned.then(|| path.to_owned()),
    }))
}

/// The youtube.com and google.com cookies of a Netscape cookie file, which
/// must be private: owned by this user and closed to everyone else (mode
/// 0600 or 0400), since it holds the session.
fn cookie_file_cookies(path: &Path) -> Result<Vec<Cookie>> {
    use std::io::Read;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let shown = format!("~/.config/{COOKIE_DIR}/{name}");
    let mut file = std::fs::File::open(path).with_context(|| format!("opening {shown}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // The open file's own metadata: it can't be swapped after the check.
        let meta = file.metadata()?;
        let me = std::fs::metadata("/proc/self").map(|m| m.uid()).ok();
        if me.is_some_and(|me| meta.uid() != me) {
            bail!("{shown} belongs to another user. Copy it again as yourself, then Reconnect.");
        }
        if meta.mode() & 0o077 != 0 {
            bail!("{shown} can be read by other users. Run `chmod 600 {shown}`, then Reconnect.");
        }
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .with_context(|| format!("reading {shown}"))?;
    Ok(parse_netscape(&text))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests copy synthetic bytes, never browser data"
)]
mod tests {
    use super::*;

    #[test]
    fn only_encores_owned_cookie_slots_are_persisted() {
        let dir = std::env::temp_dir().join(format!(
            "encore-cookie-slots-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        std::fs::create_dir(&dir).unwrap();
        for name in [IMPORTED, PASTED, "browser-cookies.txt"] {
            let path = dir.join(name);
            let session = Session {
                source: "Synthetic import".into(),
                profile: "synthetic".into(),
                cookies: parse_cookie_header("SAPISID=synthetic"),
                persist: None,
            };
            session.write_netscape(&path).unwrap();
            let candidate = Candidate {
                store: Store::CookieFile,
                id: format!("{COOKIE_DIR}/{name}"),
                label: "Synthetic import".into(),
                cookies: path.clone(),
                modified: SystemTime::UNIX_EPOCH,
                default: false,
            };
            let loaded = read_cookie_file(&candidate, &path).unwrap().unwrap();
            assert_eq!(loaded.persist.is_some(), name != "browser-cookies.txt");
            #[cfg(unix)]
            if name == IMPORTED {
                let alias = dir.join(format!("alias-{name}"));
                std::fs::rename(&path, &alias).unwrap();
                std::os::unix::fs::symlink(&alias, &path).unwrap();
                assert!(
                    read_cookie_file(&candidate, &path)
                        .unwrap()
                        .unwrap()
                        .persist
                        .is_none()
                );
            }
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn snapshots_are_private_and_cleanup_only_removes_owned_files() {
        let dir = std::env::temp_dir().join(format!("encore-snapshot-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("source");
        let copy = dir.join("copy");
        std::fs::write(&source, "synthetic bytes").unwrap();
        {
            let mut files = Copies::default();
            files.copy(&source, &copy).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
            assert_eq!(std::fs::read(&copy).unwrap(), b"synthetic bytes");
            let mut other = Copies::default();
            assert!(other.copy(&source, &copy).is_err());
            drop(other);
            assert!(copy.exists());
        }
        assert!(!copy.exists());
        assert!(source.exists());
        std::fs::remove_file(source).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
