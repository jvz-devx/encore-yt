//! Downloads a release's asset for this installation into a new staging
//! folder, hashing it on the way, and keeps it only if it matches the
//! release's `checksums.txt`.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

use super::feed::{self, Asset, Release};
use super::install::Install;
use super::job::{self, Job, Kind};

const CHECKSUMS: &str = "checksums.txt";

/// Bytes received and expected, read by the UI while it downloads.
#[derive(Default)]
pub struct Progress {
    pub received: AtomicU64,
    pub total: AtomicU64,
}

/// The verified download as a job for the helper (not written yet).
pub async fn download(
    http: &reqwest::Client,
    release: &Release,
    install: &Install,
    progress: Arc<Progress>,
) -> Result<Job> {
    let (kind, target) = match install {
        Install::AppImage(path) => (Kind::AppImage, path),
        Install::Windows(path) => (Kind::Windows, path),
        Install::Mac(path) => (Kind::Mac, path),
        Install::Manual(_) => bail!("This copy of Music can't update itself"),
    };
    let name = install
        .asset(&release.version)
        .context("No download for this system")?;
    let asset = release
        .asset(&name)
        .with_context(|| format!("The release has no {name}"))?;
    let sums = release
        .asset(CHECKSUMS)
        .context("The release has no checksums, so Music won't install it")?;
    let sums = text(http, sums).await?;
    let sha256 = feed::checksum_for(&sums, &name)
        .with_context(|| format!("The release's checksums don't list {name}"))?;
    let parent = target.parent().context("No folder to update in")?;
    let dir = job::new_staging(parent)?;
    let payload = dir.join(&name);
    progress.total.store(asset.size, Ordering::Relaxed);
    let fetched = fetch(http, asset, &payload, &progress).await;
    let verdict = fetched.and_then(|got| {
        ensure!(
            got == sha256,
            "The download doesn't match the release's checksum, so Music didn't install it"
        );
        Ok(())
    });
    if let Err(e) = verdict {
        remove(&dir);
        return Err(e);
    }
    log::info!(
        "update: {name} downloaded to {} (sha256 {sha256})",
        dir.display()
    );
    Ok(Job {
        kind,
        target: target.clone(),
        dir,
        payload,
        sha256,
        version: release.version.clone(),
    })
}

async fn text(http: &reqwest::Client, asset: &Asset) -> Result<String> {
    let response = get(http, asset).await?;
    response
        .text()
        .await
        .context("Couldn't download the checksums")
}

async fn get(http: &reqwest::Client, asset: &Asset) -> Result<reqwest::Response> {
    let url = &asset.browser_download_url;
    ensure!(
        !feed::is_github() || url.starts_with("https://"),
        "The release links to {url}, which isn't https"
    );
    let response = http
        .get(url)
        .send()
        .await
        .with_context(|| format!("Couldn't download {}", asset.name))?;
    ensure!(
        response.status().is_success(),
        "Couldn't download {}: {}",
        asset.name,
        response.status()
    );
    Ok(response)
}

/// Streams the asset into `path`; its SHA-256.
async fn fetch(
    http: &reqwest::Client,
    asset: &Asset,
    path: &Path,
    progress: &Progress,
) -> Result<String> {
    let mut response = get(http, asset).await?;
    if let Some(total) = response.content_length() {
        progress.total.store(total, Ordering::Relaxed);
    }
    let mut file = std::fs::File::create(path)
        .with_context(|| format!("Couldn't write {}", path.display()))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("The download of {} broke off", asset.name))?
    {
        hasher.update(&chunk);
        file.write_all(&chunk)?;
        progress
            .received
            .fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.sync_all()?;
    Ok(job::hex(&hasher.finalize()))
}

pub fn remove(dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir) {
        log::warn!("update: couldn't remove {}: {e}", dir.display());
    }
}
