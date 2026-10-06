//! Reads the YouTube session from the desktop's Chromium-family or Firefox
//! browser.
//!
//! The browser keeps its cookie store open, so the database is copied first.
//! Chromium values are encrypted with a key derived from the browser's "Safe
//! Storage" password, kept in the Secret Service or, on KDE, in KWallet (see
//! docs/integration.md); Firefox stores them in the clear. Cookie values and
//! that password are secrets: nothing here logs them.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use anyhow::{Context, Result, anyhow, bail};
use sha2::Digest;

/// One browser installation ytfast can read: its name, its config directory
/// under `~/.config`, the `application` its Safe Storage password is filed
/// under in the Secret Service, and its name in KWallet (folder "<name> Keys",
/// entry "<name> Safe Storage", as yt-dlp reads them).
struct Browser {
    name: &'static str,
    dir: &'static str,
    keyring: &'static str,
    kwallet: &'static str,
}

const BROWSERS: &[Browser] = &[
    Browser {
        name: "Brave Origin",
        dir: "BraveSoftware/Brave-Origin",
        keyring: "brave",
        kwallet: "Brave",
    },
    Browser {
        name: "Brave",
        dir: "BraveSoftware/Brave-Browser",
        keyring: "brave",
        kwallet: "Brave",
    },
    Browser {
        name: "Google Chrome",
        dir: "google-chrome",
        keyring: "chrome",
        kwallet: "Chrome",
    },
    Browser {
        name: "Chromium",
        dir: "chromium",
        keyring: "chromium",
        kwallet: "Chromium",
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
    with_copy(candidate, scratch, |copy| match candidate.store {
        Store::Chromium(browser) => read_copy(candidate, browser, copy),
        Store::Firefox => read_firefox_copy(candidate, copy),
    })
}

/// Runs `read` on a private copy of the profile's cookie database (with its
/// write-ahead log), removed afterwards.
fn with_copy<T>(
    candidate: &Candidate,
    scratch: &Path,
    read: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    let copy = scratch.join(format!("cookies-{}.sqlite", std::process::id()));
    let copy_wal = wal_of(&copy);
    std::fs::copy(&candidate.cookies, &copy).context("copying the cookie database")?;
    let wal = wal_of(&candidate.cookies);
    if wal.exists() {
        let _ = std::fs::copy(&wal, &copy_wal);
    }
    let result = read(&copy);
    let _ = std::fs::remove_file(&copy);
    let _ = std::fs::remove_file(&copy_wal);
    result
}

/// What ytfast can read from one browser profile, to diagnose sign-in.
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
    /// `v12` values (the desktop portal's key), which ytfast doesn't read.
    pub portal: usize,
    /// Whether the cookies read include the one sign-in needs (SAPISID).
    pub signed_in: bool,
    /// Where the Safe Storage password came from ("KWallet"), if needed.
    pub password: Option<&'static str>,
    /// Why the profile couldn't be read at all.
    pub error: Option<String>,
}

/// Reads every profile ytfast would consider, signed in or not, and
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

fn decrypt_rows(browser: &Browser, version: i64, rows: Vec<Row>) -> Result<Decrypted> {
    let needs_password = rows.iter().any(|r| r.3.starts_with(b"v11"));
    let (v11, password) = if needs_password {
        match safe_storage_password(browser)? {
            Some((password, source)) => (Some(derive_key(&password)), Some(source)),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    let keys = Keys {
        // Chromium's fixed password without a keyring (`v10`).
        v10: derive_key(b"peanuts"),
        v11,
    };
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
                "{} keeps its sign-in encrypted with the desktop portal's key, which ytfast can't read",
                candidate.label
            );
        }
        bail!(
            "{failed} cookies of {} could not be decrypted; its Safe Storage password {}",
            candidate.label,
            if password.is_some() {
                "did not match"
            } else {
                "is in neither the Secret Service nor KWallet"
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
    }))
}

/// The browser's "Safe Storage" password and where it came from: the Secret
/// Service (libsecret), else KWallet. `None` when neither has one.
fn safe_storage_password(browser: &Browser) -> Result<Option<(Vec<u8>, &'static str)>> {
    if let Some(password) = secret_service_password(browser.keyring) {
        return Ok(Some((password, "the Secret Service")));
    }
    match kwallet::password(browser.kwallet) {
        Ok(Some(password)) => Ok(Some((password, "KWallet"))),
        Ok(None) => Ok(None),
        Err(error) => Err(error.context("reading KWallet")),
    }
}

/// `secret-tool lookup application <application>`, without the trailing
/// newline. `None` when it is empty or `secret-tool` is missing.
fn secret_service_password(application: &str) -> Option<Vec<u8>> {
    let output = Command::new("secret-tool")
        .args(["lookup", "application", application])
        .output()
        .ok()?;
    let mut password = output.stdout;
    while password.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        password.pop();
    }
    Some(password).filter(|p| !p.is_empty())
}

/// KDE's wallet over D-Bus (`org.kde.KWallet`), as Chromium uses it on KDE.
mod kwallet {
    use anyhow::{Result, bail};
    use zbus::blocking::{Connection, Proxy};

    /// How ytfast names itself to kwalletd (its access prompt shows it).
    const APP_ID: &str = "ytfast";

    /// kwalletd6 (Plasma 6), then kwalletd5.
    const DAEMONS: &[(&str, &str)] = &[
        ("org.kde.kwalletd6", "/modules/kwalletd6"),
        ("org.kde.kwalletd5", "/modules/kwalletd5"),
    ];

    /// The password in folder "<name> Keys", entry "<name> Safe Storage" of
    /// the network wallet. `None` when no wallet daemon runs or the entry
    /// is missing or empty.
    pub fn password(name: &str) -> Result<Option<Vec<u8>>> {
        let Ok(bus) = Connection::session() else {
            return Ok(None);
        };
        for (service, path) in DAEMONS {
            let Ok(proxy) = Proxy::new(&bus, *service, *path, "org.kde.KWallet") else {
                continue;
            };
            // No such daemon (and none to start): try the next.
            let Ok(enabled) = proxy.call::<_, _, bool>("isEnabled", &()) else {
                continue;
            };
            if !enabled {
                return Ok(None);
            }
            let wallet = proxy
                .call::<_, _, String>("networkWallet", &())
                .ok()
                .filter(|w| !w.is_empty())
                .unwrap_or_else(|| "kdewallet".into());
            return read(&proxy, &wallet, name);
        }
        Ok(None)
    }

    fn read(proxy: &Proxy<'_>, wallet: &str, name: &str) -> Result<Option<Vec<u8>>> {
        // Asks the person to unlock the wallet if it is closed.
        let handle: i32 = proxy.call("open", &(wallet, 0i64, APP_ID))?;
        if handle < 0 {
            bail!("the wallet “{wallet}” didn't open");
        }
        let folder = format!("{name} Keys");
        let entry = format!("{name} Safe Storage");
        let found = proxy
            .call::<_, _, bool>(
                "hasEntry",
                &(handle, folder.as_str(), entry.as_str(), APP_ID),
            )
            .unwrap_or(false);
        let password = if found {
            proxy.call::<_, _, String>(
                "readPassword",
                &(handle, folder.as_str(), entry.as_str(), APP_ID),
            )
        } else {
            Ok(String::new())
        };
        let _ = proxy.call::<_, _, i32>("close", &(handle, false, APP_ID));
        Ok(Some(password?.into_bytes()).filter(|p| !p.is_empty()))
    }
}

/// The keys a Chromium profile's values are encrypted with: `v10` with the
/// fixed password, `v11` with the Safe Storage one.
struct Keys {
    v10: [u8; 16],
    v11: Option<[u8; 16]>,
}

fn derive_key(password: &[u8]) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", 1, &mut key);
    key
}

fn decrypt(encrypted: &[u8], keys: &Keys, host: &str, version: i64) -> Option<String> {
    let key = match encrypted.get(..3)? {
        b"v10" => &keys.v10,
        b"v11" => keys.v11.as_ref()?,
        _ => return None,
    };
    let body = &encrypted[3..];
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
