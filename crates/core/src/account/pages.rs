//! Idempotent optimistic edits of cached library and playlist pages.

use crate::model::{Item, ItemKind, Page, Run, Shelf, ShelfStyle, Target, Track};

/// A change to cached pages that YouTube Music may not list yet. It is
/// applied when made, and again to each copy of an affected page that
/// arrives while the change is in flight or recent: a fetch that YouTube
/// Music answered from the older state must not undo it on screen.
/// Applying one twice changes nothing.
#[derive(Clone, Debug)]
pub(super) enum PageEdit {
    /// A Library card for a playlist being created (`id` once known).
    Create {
        title: String,
        thumbnail: Option<String>,
        id: Option<String>,
    },
    /// Songs at the end of the playlist (with their entry ids once known).
    Add {
        playlist_id: String,
        tracks: Vec<Track>,
    },
    Remove {
        playlist_id: String,
        set_video_id: String,
    },
    /// An entry moved before another (to the end when `before` is `None`).
    Move {
        playlist_id: String,
        set_video_id: String,
        before: Option<String>,
    },
    Details {
        playlist_id: String,
        title: String,
        description: Option<String>,
    },
    Delete {
        playlist_id: String,
    },
}

pub(super) fn playlist_target(playlist_id: &str) -> Target {
    Target::browse(format!("VL{playlist_id}"))
}

/// Library tabs' pages: Playlists
/// here; Songs, Albums and Artists are `FEmusic_liked_videos`,
/// `FEmusic_liked_albums` and `FEmusic_library_corpus_track_artists`.
pub(super) fn library_target() -> Target {
    Target::browse("FEmusic_liked_playlists")
}

/// A row for a song added to a playlist page before YouTube Music lists it.
fn row_for(track: &Track) -> Item {
    let mut subtitle = track.artists.clone();
    if let Some(album) = &track.album {
        if !subtitle.is_empty() {
            subtitle.push(Run {
                text: " • ".into(),
                target: None,
            });
        }
        subtitle.push(album.clone());
    }
    Item {
        kind: ItemKind::Song,
        title: track.title.clone(),
        subtitle,
        thumbnail: track.thumbnail.clone(),
        target: Some(Target::Watch {
            video_id: Some(track.video_id.clone()),
            playlist_id: None,
            params: None,
        }),
        play: None,
        track: Some(track.clone()),
        index: None,
        stripe: None,
        editable: None,
    }
}

pub(super) fn entry_of(item: &Item) -> Option<&str> {
    item.track.as_ref()?.set_video_id.as_deref()
}

fn is_card_of(item: &Item, playlist_id: &str) -> bool {
    item.editable.as_deref() == Some(playlist_id)
        || matches!(&item.target, Some(Target::Browse { id, .. }) if id.strip_prefix("VL") == Some(playlist_id))
}

/// A card added here for a playlist that YouTube Music doesn't list yet.
fn is_placeholder(item: &Item, title: &str) -> bool {
    item.title == title && item.target.is_none() && item.editable.is_none()
}

/// The playlist's own songs: its first untitled list. YouTube Music adds a
/// "Suggestions" list after it.
pub fn entries(page: &Page) -> Option<&Shelf> {
    page.shelves
        .iter()
        .find(|s| s.style == ShelfStyle::List && s.title.is_empty())
}

fn entries_mut(page: &mut Page) -> &mut Shelf {
    let at = page
        .shelves
        .iter()
        .position(|s| s.style == ShelfStyle::List && s.title.is_empty());
    let at = at.unwrap_or_else(|| {
        page.shelves.insert(
            0,
            Shelf {
                title: String::new(),
                strapline: None,
                style: ShelfStyle::List,
                items: Vec::new(),
                more: None,
                continuation: None,
            },
        );
        0
    });
    &mut page.shelves[at]
}

pub(super) fn is_playlist_page(key: &str, page: &Page, playlist_id: &str) -> bool {
    key == playlist_target(playlist_id).key()
        || page
            .header
            .as_ref()
            .is_some_and(|h| h.editable.as_deref() == Some(playlist_id))
}

impl PageEdit {
    pub(super) fn playlist(&self) -> Option<&str> {
        match self {
            PageEdit::Create { .. } => None,
            PageEdit::Add { playlist_id, .. }
            | PageEdit::Remove { playlist_id, .. }
            | PageEdit::Move { playlist_id, .. }
            | PageEdit::Details { playlist_id, .. }
            | PageEdit::Delete { playlist_id } => Some(playlist_id),
        }
    }

    /// Whether the page cached under `key` is one the change affects.
    pub(super) fn affects(&self, key: &str, page: &Page) -> bool {
        key == library_target().key()
            || self
                .playlist()
                .is_some_and(|id| is_playlist_page(key, page, id))
    }

    /// Applies the change to the page cached under `key`, if it is one the
    /// change affects.
    pub(super) fn apply(&self, key: &str, page: &mut Page) {
        if key == library_target().key() {
            self.apply_to_library(page);
        }
        if let Some(id) = self.playlist()
            && is_playlist_page(key, page, id)
        {
            self.apply_to_playlist(page);
        }
    }

    fn apply_to_library(&self, page: &mut Page) {
        match self {
            PageEdit::Create {
                title,
                thumbnail,
                id,
            } => {
                let listed = page
                    .shelves
                    .iter()
                    .flat_map(|s| &s.items)
                    .any(|i| id.as_deref().is_some_and(|id| is_card_of(i, id)));
                if listed {
                    return;
                }
                if let Some(card) = page
                    .shelves
                    .iter_mut()
                    .flat_map(|s| &mut s.items)
                    .find(|i| is_placeholder(i, title))
                {
                    if let Some(id) = id {
                        card.target = Some(playlist_target(id));
                        card.editable = Some(id.clone());
                    }
                    return;
                }
                let card = Item {
                    kind: ItemKind::Playlist,
                    title: title.clone(),
                    subtitle: vec![Run {
                        text: "Playlist".into(),
                        target: None,
                    }],
                    thumbnail: thumbnail.clone(),
                    target: id.as_deref().map(playlist_target),
                    play: None,
                    track: None,
                    index: None,
                    stripe: None,
                    editable: id.clone(),
                };
                match page.shelves.first_mut() {
                    // YouTube Music lists a new playlist first among the
                    // account's own, after the automatic ones.
                    Some(shelf) => {
                        let at = shelf
                            .items
                            .iter()
                            .position(|i| i.editable.is_some())
                            .unwrap_or(shelf.items.len().min(2));
                        shelf.items.insert(at, card);
                    }
                    None => page.shelves.push(Shelf {
                        title: String::new(),
                        strapline: None,
                        style: ShelfStyle::Grid,
                        items: vec![card],
                        more: None,
                        continuation: None,
                    }),
                }
            }
            PageEdit::Details {
                playlist_id, title, ..
            } => {
                let cards = page.shelves.iter_mut().flat_map(|s| &mut s.items);
                for card in cards.filter(|c| is_card_of(c, playlist_id)) {
                    card.title = title.clone();
                }
            }
            PageEdit::Delete { playlist_id } => {
                for shelf in &mut page.shelves {
                    shelf.items.retain(|i| !is_card_of(i, playlist_id));
                }
            }
            PageEdit::Add { .. } | PageEdit::Remove { .. } | PageEdit::Move { .. } => {}
        }
    }

    fn apply_to_playlist(&self, page: &mut Page) {
        match self {
            PageEdit::Add { tracks, .. } => {
                page.message = None;
                let shelf = entries_mut(page);
                for track in tracks {
                    let row = shelf.items.iter_mut().find_map(|i| {
                        i.track.as_mut().filter(|t| {
                            t.video_id == track.video_id
                                && (t.set_video_id.is_none()
                                    || t.set_video_id == track.set_video_id)
                        })
                    });
                    match row {
                        Some(listed) => {
                            if listed.set_video_id.is_none() {
                                listed.set_video_id = track.set_video_id.clone();
                            }
                        }
                        None => shelf.items.push(row_for(track)),
                    }
                }
            }
            PageEdit::Remove { set_video_id, .. } => {
                for shelf in &mut page.shelves {
                    shelf
                        .items
                        .retain(|i| entry_of(i) != Some(set_video_id.as_str()));
                }
            }
            PageEdit::Move {
                set_video_id,
                before,
                ..
            } => {
                let shelf = entries_mut(page);
                let Some(from) = shelf
                    .items
                    .iter()
                    .position(|i| entry_of(i) == Some(set_video_id.as_str()))
                else {
                    return;
                };
                let item = shelf.items.remove(from);
                let to = match before {
                    Some(before) => shelf
                        .items
                        .iter()
                        .position(|i| entry_of(i) == Some(before.as_str())),
                    None => Some(shelf.items.len()),
                };
                // The entry it went before is gone: leave it where it was.
                shelf.items.insert(to.unwrap_or(from), item);
            }
            PageEdit::Details {
                title, description, ..
            } => {
                if let Some(h) = page.header.as_mut() {
                    h.title = title.clone();
                    h.description = description.clone();
                }
            }
            PageEdit::Create { .. } | PageEdit::Delete { .. } => {}
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "test assertions inspect synthetic playlist rows"
)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            video_id: id.into(),
            title: id.into(),
            artists: Vec::new(),
            album: None,
            thumbnail: None,
            duration: None,
            like: None,
            set_video_id: Some(id.into()),
        }
    }

    #[test]
    fn playlist_overlays_are_idempotent_and_preserve_entry_order() {
        let key = playlist_target("test").key();
        let mut page = Page::default();
        let add = PageEdit::Add {
            playlist_id: "test".into(),
            tracks: vec![track("a"), track("b")],
        };
        add.apply(&key, &mut page);
        add.apply(&key, &mut page);
        assert_eq!(entries(&page).unwrap().items.len(), 2);
        let reorder = PageEdit::Move {
            playlist_id: "test".into(),
            set_video_id: "b".into(),
            before: Some("a".into()),
        };
        reorder.apply(&key, &mut page);
        reorder.apply(&key, &mut page);
        let order: Vec<_> = entries(&page)
            .unwrap()
            .items
            .iter()
            .filter_map(entry_of)
            .collect();
        assert_eq!(order, ["b", "a"]);
        let remove = PageEdit::Remove {
            playlist_id: "test".into(),
            set_video_id: "b".into(),
        };
        remove.apply(&key, &mut page);
        remove.apply(&key, &mut page);
        assert_eq!(entries(&page).unwrap().items.len(), 1);
        assert_eq!(entry_of(&entries(&page).unwrap().items[0]), Some("a"));
    }

    #[test]
    fn an_overlay_does_not_touch_an_unrelated_playlist() {
        let mut page = Page::default();
        let edit = PageEdit::Add {
            playlist_id: "first".into(),
            tracks: vec![track("a")],
        };
        let key = playlist_target("second").key();
        assert!(!edit.affects(&key, &page));
        edit.apply(&key, &mut page);
        assert_eq!(page, Page::default());
    }
}
