//! Download and verify pinned solver releases before making them available.

use crate::jsc::scripts;
use anyhow::{Context, Result, bail};
use std::path::Path;

/// The pins file on the repository's default branch: the list of solver
/// releases (by SHA-256) the app may download when its own solver can't
/// use a player. Changing it takes a reviewed commit to `main`.
const PINS_URL: &str =
    "https://raw.githubusercontent.com/jvz-devx/encore-yt/main/crates/core/src/jsc/pins.txt";

/// Where yt-dlp-ejs publishes its release assets.
const EJS_RELEASES: &str = "https://github.com/yt-dlp/ejs/releases/download";

/// Fetches the repository's pins and, if they name a release newer than
/// `current`, downloads and checks its two files and saves them with the
/// pins under `dir`.
pub(super) async fn fetch_solver(
    http: &reqwest::Client,
    dir: &Path,
    current: &str,
) -> Result<Option<scripts::Scripts>> {
    let get = |url: String| async move {
        http.get(&url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .with_context(|| format!("fetching {url}"))?
            .text()
            .await
            .with_context(|| format!("reading {url}"))
    };
    let fetched = get(PINS_URL.to_owned()).await?;
    let mut pins = scripts::parse_pins(scripts::PINS);
    pins.extend(scripts::parse_pins(&fetched));
    let Some(version) = scripts::newest_pinned(&pins, current) else {
        return Ok(None);
    };
    let mut files = Vec::new();
    for name in [scripts::LIB, scripts::CORE] {
        let text = get(format!("{EJS_RELEASES}/{version}/{name}")).await?;
        let want = scripts::pinned_hash(&pins, &version, name)?;
        if scripts::sha256_hex(text.as_bytes()) != want {
            bail!("{name} of EJS {version} doesn't match its pinned hash");
        }
        files.push(text);
    }
    let core = files.pop().context("missing downloaded solver core")?;
    let lib = files.pop().context("missing downloaded solver library")?;
    let found = scripts::Scripts::verified(lib, core, &pins)?;
    tokio::fs::create_dir_all(dir)
        .await
        .context("creating the solver directory")?;
    for (name, text) in [
        (scripts::LIB, &*found.lib),
        (scripts::CORE, &*found.core),
        (scripts::PINS_FILE, fetched.as_str()),
    ] {
        crate::paths::write_atomic_async(dir.join(name), text.as_bytes().to_vec())
            .await
            .with_context(|| format!("saving {name}"))?;
    }
    log::info!("downloaded EJS solver {version}");
    Ok(Some(found))
}
