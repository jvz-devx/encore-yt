//! Refreshes the parser fixtures in `tests/fixtures/innertube/`: signed-out
//! `WEB_REMIX` responses captured with Encore's own client (no cookies), with
//! the tracking and session parts the parser never reads removed, and song
//! lyrics cut to their first lines.
//!
//! `cargo run -p encore-core --example capture_fixtures`, then
//! `cargo test --test parse_fixtures`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use encore_core::innertube::Client;
use encore_core::model::{ItemKind, Page, Target};
use encore_core::parse;
use serde::Serialize;
use serde_json::Value;

/// Keys dropped everywhere: tracking, logging and session data, and
/// accessibility labels that repeat the visible text.
const DROPPED: &[&str] = &[
    "responseContext",
    "trackingParams",
    "clickTrackingParams",
    "visitorData",
    "loggingContext",
    "loggingDirectives",
    "frameworkUpdates",
    "adSignalsInfo",
    "serviceTrackingParams",
    "accessibility",
    "accessibilityData",
];

/// Lyric lines kept (the parser's structure, not the song's words).
const LYRIC_LINES: usize = 3;

/// Cards and rows kept per shelf, queue or grid: enough to show their
/// shape, while a mood page alone would otherwise be megabytes.
const ITEMS: usize = 6;

/// Menu entries the parser never reads (add to queue, feedback, download).
const MENU_SERVICES: &[&str] = &[
    "menuServiceItemRenderer",
    "toggleMenuServiceItemRenderer",
    "menuServiceItemDownloadRenderer",
];

/// The renderers of a shelf's cards and rows.
const ITEM_RENDERERS: &[&str] = &[
    "musicTwoRowItemRenderer",
    "musicResponsiveListItemRenderer",
    "musicNavigationButtonRenderer",
    "musicMultiRowListItemRenderer",
    "playlistPanelVideoRenderer",
    "playlistPanelVideoWrapperRenderer",
];

fn main() -> Result<()> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/innertube");
    std::fs::create_dir_all(&dir)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(capture(&dir))
}

async fn capture(dir: &Path) -> Result<()> {
    let client = Client::new();
    save(dir, "home", client.browse("FEmusic_home", None).await?)?;
    save(
        dir,
        "explore",
        client.browse("FEmusic_explore", None).await?,
    )?;
    let search = client.search("daft punk", None).await?;
    let found = parse::page(&search);
    save(dir, "search", search)?;

    let artist = browse_of(&found, ItemKind::Artist, "UC")?;
    save(dir, "artist", client.browse(&artist, None).await?)?;
    let album = browse_of(&found, ItemKind::Album, "MPRE")?;
    save(dir, "album", client.browse(&album, None).await?)?;
    let playlist = browse_of(&found, ItemKind::Playlist, "VL")?;
    save(dir, "playlist", client.browse(&playlist, None).await?)?;

    let moods = parse::page(&client.browse("FEmusic_moods_and_genres", None).await?);
    let mood = moods
        .shelves
        .iter()
        .flat_map(|s| &s.items)
        .find_map(|i| match &i.target {
            Some(Target::Browse { id, params }) if params.is_some() => {
                Some((id.clone(), params.clone()))
            }
            _ => None,
        })
        .ok_or_else(|| anyhow!("no mood on Moods & genres"))?;
    save(
        dir,
        "mood",
        client.browse(&mood.0, mood.1.as_deref()).await?,
    )?;

    let song = found
        .shelves
        .iter()
        .flat_map(|s| &s.items)
        .find_map(|i| i.track.as_ref().filter(|_| i.kind == ItemKind::Song))
        .context("no song in the search results")?;
    let next = client
        .next(&Target::Watch {
            video_id: Some(song.video_id.clone()),
            playlist_id: None,
            params: None,
        })
        .await?;
    let lyrics = parse::watch_next(&next).lyrics;
    save(dir, "next", next)?;
    if let Some(lyrics) = lyrics {
        save(dir, "lyrics", client.browse(&lyrics, None).await?)?;
        save(dir, "lyrics_timed", client.timed_lyrics(&lyrics).await?)?;
    }
    Ok(())
}

/// The browse id of the first `kind` item in the results whose id starts
/// with `prefix`.
fn browse_of(page: &Page, kind: ItemKind, prefix: &str) -> Result<String> {
    page.shelves
        .iter()
        .flat_map(|s| &s.items)
        .filter(|i| i.kind == kind)
        .find_map(|i| match &i.target {
            Some(Target::Browse { id, .. }) if id.starts_with(prefix) => Some(id.clone()),
            _ => None,
        })
        .ok_or_else(|| anyhow!("no {kind:?} in the search results"))
}

fn save(dir: &Path, name: &str, mut value: Value) -> Result<()> {
    strip(&mut value);
    let path: PathBuf = dir.join(format!("{name}.json"));
    // One value per line without indentation: diffable, and a fraction of
    // the size of indented JSON at YouTube's nesting depth.
    let mut text = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"");
    value.serialize(&mut serde_json::Serializer::with_formatter(
        &mut text, formatter,
    ))?;
    text.push(b'\n');
    std::fs::write(&path, &text)?;
    println!("{name}: {} KB", text.len() / 1024);
    Ok(())
}

fn strip(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !DROPPED.contains(&key.as_str()));
            if let Some(Value::Array(lines)) = map.get_mut("timedLyricsData") {
                lines.truncate(LYRIC_LINES);
            }
            if let Some(shelf) = map.get_mut("musicDescriptionShelfRenderer") {
                shorten_description(shelf);
            }
            map.values_mut().for_each(strip);
        }
        Value::Array(items) => {
            items.retain(|i| !has_key(i, MENU_SERVICES));
            if items.iter().all(|i| has_key(i, ITEM_RENDERERS)) {
                items.truncate(ITEMS);
            }
            items.iter_mut().for_each(strip)
        }
        _ => {}
    }
}

/// Whether `value` is an object with one of `keys` (a renderer of that kind).
fn has_key(value: &Value, keys: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|o| o.keys().any(|k| keys.contains(&k.as_str())))
}

/// Plain lyrics (and artist biographies) cut to their first lines.
fn shorten_description(shelf: &mut Value) {
    let Some(Value::Array(runs)) = shelf.pointer_mut("/description/runs") else {
        return;
    };
    runs.truncate(1);
    if let Some(Value::String(text)) = runs.first_mut().and_then(|r| r.get_mut("text")) {
        *text = text
            .lines()
            .take(LYRIC_LINES)
            .collect::<Vec<_>>()
            .join("\n");
    }
}
