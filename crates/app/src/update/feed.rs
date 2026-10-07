//! The release feed: GitHub's release list for the repository, or
//! `YTFAST_UPDATE_FEED` (the same JSON from anywhere, for testing), and the
//! newest release above this version.

use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;

const FEED: &str = "https://api.github.com/repos/jvz-devx/ytfast-gpui/releases?per_page=30";

/// A release newer than this build.
#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// The release's description (Markdown, as written on GitHub).
    pub notes: String,
    /// Its page on GitHub.
    pub page: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize)]
struct Listed {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The URL of the release list.
pub fn url() -> String {
    std::env::var("YTFAST_UPDATE_FEED").unwrap_or_else(|_| FEED.to_string())
}

/// Whether the feed is the real one (downloads must then be https).
pub fn is_github() -> bool {
    std::env::var_os("YTFAST_UPDATE_FEED").is_none()
}

/// The newest release above `current`; pre-releases only with
/// `prereleases`.
pub async fn check(
    http: &reqwest::Client,
    current: &str,
    prereleases: bool,
) -> Result<Option<Release>> {
    let response = http
        .get(url())
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Couldn't reach GitHub")?;
    if !response.status().is_success() {
        bail!("GitHub answered {}", response.status());
    }
    let bytes = response
        .bytes()
        .await
        .context("Couldn't read the releases")?;
    let listed: Vec<Listed> =
        serde_json::from_slice(&bytes).context("Couldn't read the releases")?;
    let current = Version::parse(current).context("This build's version isn't valid")?;
    Ok(newest(listed, &current, prereleases))
}

fn newest(listed: Vec<Listed>, current: &Version, prereleases: bool) -> Option<Release> {
    listed
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.strip_prefix('v')?).ok()?;
            let pre = r.prerelease || !version.pre.is_empty();
            (version > *current && (prereleases || !pre)).then_some((version, r))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(version, r)| Release {
            version: version.to_string(),
            notes: r.body.unwrap_or_default(),
            page: r.html_url,
            assets: r.assets,
        })
}

/// `checksums.txt` in `sha256sum` format: the hash for `name`.
pub fn checksum_for(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// Whether `version` is a pre-release (`0.1.0-alpha.2`).
pub fn is_prerelease(version: &str) -> bool {
    Version::parse(version).is_ok_and(|v| !v.pre.is_empty())
}
