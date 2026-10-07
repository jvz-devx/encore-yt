//! Which EJS solver scripts the engine runs: the copy vendored in the app,
//! or a newer release saved on disk whose files' SHA-256 hashes are pinned.
//!
//! Pins come from `pins.txt` (shipped in the app) and from the copy of that
//! file last fetched from the repository (`<cache>/ejs/pins.txt`). Nothing
//! unpinned ever runs: a file whose hash isn't listed, or a lib and core
//! from different releases, is ignored and the vendored copy runs instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use sha2::Digest as _;

/// The pins shipped in the app.
pub const PINS: &str = include_str!("pins.txt");
/// The release asset names, which are also the file names on disk.
pub const LIB: &str = "yt.solver.lib.min.js";
pub const CORE: &str = "yt.solver.core.min.js";
/// Where the app keeps the solver it downloaded and the fetched pins.
pub const DIR: &str = "ejs";
pub const PINS_FILE: &str = "pins.txt";

const VENDORED_LIB: &str = include_str!("ejs-lib.min.js");
const VENDORED_CORE: &str = include_str!("ejs-core.min.js");

/// One pinned file: its hash, release and asset name.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub sha256: String,
    pub version: String,
    pub file: String,
}

/// Reads a pins file: `<sha256>  <version>  <file>` per line, `#` comments.
pub fn parse_pins(text: &str) -> Vec<Pin> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (sha256, version, file) = (fields.next()?, fields.next()?, fields.next()?);
            let valid = sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit());
            valid.then(|| Pin {
                sha256: sha256.to_ascii_lowercase(),
                version: version.to_owned(),
                file: file.to_owned(),
            })
        })
        .collect()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether release `a` is newer than `b` (dotted numbers, `0.10.0 > 0.9.1`).
pub fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a) > parts(b)
}

/// A solver release's two scripts, checked against the pins.
#[derive(Clone, Debug)]
pub struct Scripts {
    pub version: String,
    pub lib: Arc<str>,
    pub core: Arc<str>,
}

impl Scripts {
    /// The copy built into the app.
    pub fn vendored() -> Self {
        let pins = parse_pins(PINS);
        let version =
            release_of(&pins, VENDORED_LIB, VENDORED_CORE).unwrap_or_else(|| "vendored".into());
        Self {
            version,
            lib: VENDORED_LIB.into(),
            core: VENDORED_CORE.into(),
        }
    }

    /// The scripts if both files are pinned to the same release.
    pub fn verified(lib: String, core: String, pins: &[Pin]) -> Result<Self> {
        let version = release_of(pins, &lib, &core)
            .context("the solver scripts' hashes aren't pinned to one release")?;
        Ok(Self {
            version,
            lib: lib.into(),
            core: core.into(),
        })
    }

    /// The scripts saved in `dir`, if they are there and pinned.
    pub fn from_dir(dir: &Path, pins: &[Pin]) -> Result<Self> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name))
                .with_context(|| format!("reading {}", dir.join(name).display()))
        };
        Self::verified(read(LIB)?, read(CORE)?, pins)
    }

    /// The newest pinned release among the vendored copy and the ones saved
    /// in `dirs` (the cache's and the config's `ejs/`), with the pins
    /// shipped in the app and the ones fetched into `fetched_pins`.
    pub fn best(dirs: &[PathBuf], fetched_pins: &Path) -> Self {
        let pins = all_pins(fetched_pins);
        let vendored = Self::vendored();
        let mut best = vendored.clone();
        for dir in dirs {
            if !dir.join(LIB).exists() {
                continue;
            }
            match Self::from_dir(dir, &pins) {
                Ok(found) if newer(&found.version, &best.version) => best = found,
                Ok(_) => {}
                Err(error) => log::warn!("ignoring the solver in {}: {error:#}", dir.display()),
            }
        }
        if best.version != vendored.version {
            log::info!("using EJS solver {} from disk", best.version);
        }
        best
    }
}

/// The shipped pins plus the fetched ones, if that file reads.
pub fn all_pins(fetched: &Path) -> Vec<Pin> {
    let mut pins = parse_pins(PINS);
    if let Ok(text) = std::fs::read_to_string(fetched) {
        pins.extend(parse_pins(&text));
    }
    pins
}

/// The newest release both files are pinned to, if any (releases often
/// share an unchanged file).
fn release_of(pins: &[Pin], lib: &str, core: &str) -> Option<String> {
    let (lib, core) = (sha256_hex(lib.as_bytes()), sha256_hex(core.as_bytes()));
    pins.iter()
        .filter(|p| p.file == LIB && p.sha256 == lib)
        .filter(|l| {
            pins.iter()
                .any(|c| c.file == CORE && c.sha256 == core && c.version == l.version)
        })
        .map(|p| p.version.clone())
        .reduce(|a, b| if newer(&b, &a) { b } else { a })
}

/// The newest release `pins` lists with both files, if newer than `than`.
pub fn newest_pinned(pins: &[Pin], than: &str) -> Option<String> {
    pins.iter()
        .filter(|p| p.file == LIB)
        .filter(|l| {
            pins.iter()
                .any(|c| c.file == CORE && c.version == l.version)
        })
        .map(|p| p.version.clone())
        .filter(|v| newer(v, than))
        .reduce(|a, b| if newer(&b, &a) { b } else { a })
}

/// The pinned hash of one file of a release.
pub fn pinned_hash<'a>(pins: &'a [Pin], version: &str, file: &str) -> Result<&'a str> {
    match pins.iter().find(|p| p.version == version && p.file == file) {
        Some(pin) => Ok(&pin.sha256),
        None => bail!("{file} of {version} isn't pinned"),
    }
}
