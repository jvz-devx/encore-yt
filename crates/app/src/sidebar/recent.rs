//! Recently played: albums, playlists and radios started from this app,
//! kept in `~/.cache/ytfast/recent.json` (written whole, atomically).
//!
//! A play target says only which playlist to play, so the title and cover
//! come from what was on screen: the page header or the card whose button
//! started it, or the song a radio was started from.

use std::path::Path;

use ytfast::model::{Header, Item, ItemKind, Run, Target, Track};

use crate::app::MusicApp;

/// How many the sidebar keeps.
const KEEP: usize = 5;

/// The playlist a target plays (an album's, a playlist's, a radio's), or
/// `None` for a page or a single song.
pub fn playlist_id(target: &Target) -> Option<&str> {
    match target {
        Target::Watch {
            playlist_id: Some(id),
            ..
        } => Some(id),
        _ => None,
    }
}

/// The same collection, whichever button started it (Play or Shuffle).
pub fn same(a: &Item, b: &Item) -> bool {
    let id = |i: &Item| i.play.as_ref().and_then(playlist_id).map(str::to_string);
    id(a).is_some() && id(a) == id(b)
}

pub fn push(list: &mut Vec<Item>, item: Item) {
    list.retain(|i| !same(i, &item));
    list.insert(0, item);
    list.truncate(KEEP);
}

pub fn load(path: &Path) -> Vec<Item> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|e| {
        log::warn!("ignoring {}: {e}", path.display());
        Vec::new()
    })
}

pub fn save(path: &Path, list: &[Item]) {
    let written = serde_json::to_vec(list)
        .map_err(std::io::Error::other)
        .and_then(|bytes| ytfast::paths::write_atomic(path, &bytes));
    if let Err(e) = written {
        log::warn!("couldn't save {}: {e}", path.display());
    }
}

/// What a collection target is, from the pages the app holds (the open one
/// first) and the queue; `None` for songs and for what it can't name.
pub fn describe(app: &MusicApp, target: &Target) -> Option<Item> {
    let playlist = playlist_id(target)?;
    let open = app.pages.view.target().key();
    let mut states: Vec<_> = app.pages.states.iter().collect();
    states.sort_by_key(|(key, _)| **key != open);
    for (_, state) in &states {
        let Some(page) = &state.page else { continue };
        if let Some(item) = page
            .header
            .as_ref()
            .and_then(|h| from_header(h, &state.target, target))
        {
            return Some(item);
        }
        let found = page
            .shelves
            .iter()
            .flat_map(|s| s.items.iter())
            .find(|i| i.play.as_ref() == Some(target) || i.target.as_ref() == Some(target));
        if let Some(item) = found {
            return Some(match &item.track {
                Some(track) => radio(track, target),
                None => entry(item.clone(), target),
            });
        }
    }
    // A song's radio from the player bar, Up next or a menu.
    let video = playlist.strip_prefix("RDAMVM")?;
    let track = app
        .player
        .queue
        .iter()
        .chain(
            states
                .iter()
                .filter_map(|(_, s)| s.page.as_ref())
                .flat_map(|p| p.shelves.iter().flat_map(|s| s.items.iter()))
                .filter_map(|i| i.track.as_ref()),
        )
        .find(|t| t.video_id == video)?;
    Some(radio(track, target))
}

/// The header's Play, Shuffle or Radio of page `page`.
fn from_header(header: &Header, page: &Target, target: &Target) -> Option<Item> {
    let radio = header.radio.as_ref() == Some(target);
    if !radio && header.play.as_ref() != Some(target) && header.shuffle.as_ref() != Some(target) {
        return None;
    }
    let kind = match page {
        Target::Browse { id, .. } if id.starts_with("MPRE") => ItemKind::Album,
        Target::Browse { id, .. } if id.starts_with("UC") => ItemKind::Artist,
        _ => ItemKind::Playlist,
    };
    let subtitle = if radio {
        vec![plain("Radio")]
    } else {
        header.subtitle.clone()
    };
    Some(Item {
        kind: if radio { ItemKind::Playlist } else { kind },
        title: header.title.clone(),
        subtitle,
        thumbnail: header.thumbnail.clone(),
        // A radio has no page of its own: choosing it plays it again.
        target: (!radio).then(|| page.clone()),
        play: Some(target.clone()),
        track: None,
        index: None,
        stripe: None,
        editable: None,
    })
}

/// A card's collection, kept with its own title, subtitle and cover.
fn entry(item: Item, target: &Target) -> Item {
    Item {
        // Whichever started it, choosing it opens its page.
        target: item.target.filter(|t| playlist_id(t).is_none()),
        play: Some(target.clone()),
        track: None,
        index: None,
        editable: None,
        ..item
    }
}

/// A song's radio.
fn radio(track: &Track, target: &Target) -> Item {
    Item {
        kind: ItemKind::Playlist,
        title: track.title.clone(),
        subtitle: vec![plain("Radio")],
        thumbnail: track.thumbnail.clone(),
        target: None,
        play: Some(target.clone()),
        track: None,
        index: None,
        stripe: None,
        editable: None,
    }
}

fn plain(text: &str) -> Run {
    Run {
        text: text.to_string(),
        target: None,
    }
}

/// Playlist page `key` itself, played as its list of songs: a page whose
/// id names its playlist (`VL…`).
pub fn from_page(app: &MusicApp, key: &str) -> Option<Item> {
    let state = app.pages.states.get(key)?;
    let header = state.page.as_ref()?.header.as_ref()?;
    let Target::Browse { id, .. } = &state.target else {
        return None;
    };
    let play = Target::Watch {
        video_id: None,
        playlist_id: Some(id.strip_prefix("VL")?.to_string()),
        params: None,
    };
    Some(Item {
        kind: ItemKind::Playlist,
        title: header.title.clone(),
        subtitle: header.subtitle.clone(),
        thumbnail: header.thumbnail.clone(),
        target: Some(state.target.clone()),
        play: Some(play),
        track: None,
        index: None,
        stripe: None,
        editable: None,
    })
}
