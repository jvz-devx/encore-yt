//! Lists the YouTube channels of the signed-in account and, acting as each
//! one, how many playlists Library shows, how many songs Liked music has
//! and how many shelves Home has: read-only, to check that requests act as the chosen
//! channel. Prints counts, never a cookie; channel names only with
//! `--names`.
//!
//! `cargo run --example channels [-- [--names] [<profile id>]]`

fn main() -> anyhow::Result<()> {
    let names = std::env::args().any(|a| a == "--names");
    let preferred = std::env::args()
        .skip(1)
        .find(|a| a != "--names" && !a.is_empty());
    let scratch = tempdir()?;
    let session = encore_core::auth::load(&scratch, preferred.as_deref());
    let _ = std::fs::remove_dir_all(&scratch);
    let session = session?;
    let client = encore_core::innertube::Client::new();
    client.set_session(Some(session));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let channels = encore_core::parse::channels(&client.channels().await?);
        println!("{} channels", channels.len());
        for (i, channel) in channels.iter().enumerate() {
            client.set_page_id(channel.page_id.clone());
            let named = match client.account().await {
                Ok(menu) => encore_core::parse::account(&menu).map(|(name, _)| name),
                Err(error) => Some(format!("({error})")),
            };
            let label = if names {
                format!("{} {}", channel.name, channel.handle.as_deref().unwrap_or("-"))
            } else {
                format!("Channel {}", i + 1)
            };
            let library = client.browse("FEmusic_liked_playlists", None).await?;
            let liked = client.browse("VLLM", None).await?;
            let home = client.browse("FEmusic_home", None).await?;
            println!(
                "{label} ({}{}): account menu {}; Library {} playlists, Liked music {} songs, Home {} shelves with {} cards",
                if channel.page_id.is_some() { "brand account" } else { "own channel" },
                if channel.current { ", selected" } else { "" },
                if named.as_deref() == Some(channel.name.as_str()) { "names it" } else { "names another" },
                count(&library),
                count(&liked),
                encore_core::parse::page(&home).shelves.len(),
                count(&home),
            );
        }
        anyhow::Ok(())
    })
}

/// Cards and rows on a page's first screen.
fn count(value: &serde_json::Value) -> usize {
    encore_core::parse::page(value)
        .shelves
        .iter()
        .map(|s| s.items.len())
        .sum()
}

/// A private directory for the cookie database copies.
fn tempdir() -> anyhow::Result<std::path::PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = std::env::temp_dir().join(format!("encore-channels-{}", std::process::id()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}
