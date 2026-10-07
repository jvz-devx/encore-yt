//! Runs saved signed-out InnerTube responses (`tests/fixtures/innertube/`,
//! refreshed with `cargo run --example capture_fixtures
//! --no-default-features`) through the real parser, so a change in YouTube
//! Music's responses shows up as a failing test. The assertions check the
//! structure the interface relies on, not the catalogue's current content.

use serde_json::Value;
use ytfast::model::{Item, ItemKind, LikeStatus, Page, Shelf, ShelfStyle, Target, Track};
use ytfast::parse;

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/innertube/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn page(name: &str) -> Page {
    parse::page(&fixture(name))
}

fn shelf<'a>(page: &'a Page, title: &str) -> &'a Shelf {
    page.shelves
        .iter()
        .find(|s| s.title == title)
        .unwrap_or_else(|| panic!("no shelf {title:?} in {:?}", titles(page)))
}

fn titles(page: &Page) -> Vec<&str> {
    page.shelves.iter().map(|s| s.title.as_str()).collect()
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A playable song row: its track has an id, a title and artists, and
/// clicking it plays that id.
fn assert_song(item: &Item) -> &Track {
    let track = item
        .track
        .as_ref()
        .unwrap_or_else(|| panic!("{:?} has no track", item.title));
    assert!(is_video_id(&track.video_id), "{:?}", track.video_id);
    assert!(!track.title.is_empty());
    assert!(
        !track.artists.is_empty(),
        "{:?} has no artists",
        track.title
    );
    match &item.target {
        Some(Target::Watch { video_id, .. }) => {
            assert_eq!(video_id.as_deref(), Some(track.video_id.as_str()))
        }
        other => panic!("{:?} plays {other:?}", item.title),
    }
    track
}

/// A card that opens a page and shows a cover.
fn assert_card(item: &Item) {
    assert!(!item.title.is_empty());
    assert!(item.thumbnail.is_some(), "{:?} has no cover", item.title);
    assert!(item.target.is_some(), "{:?} opens nothing", item.title);
}

fn browse_id(target: &Option<Target>) -> &str {
    match target {
        Some(Target::Browse { id, .. }) => id,
        other => panic!("not a browse target: {other:?}"),
    }
}

#[test]
fn home() {
    let home = page("home");
    assert!(home.shelves.len() >= 2, "{:?}", titles(&home));
    for shelf in &home.shelves {
        assert!(!shelf.title.is_empty());
        assert!(!shelf.items.is_empty(), "{:?} is empty", shelf.title);
        shelf.items.iter().for_each(assert_card);
    }
    let quick = shelf(&home, "Quick picks");
    assert_eq!(quick.style, ShelfStyle::RowCarousel);
    for item in &quick.items {
        assert_song(item);
    }
    // The mood chips, and more shelves to load.
    assert!(home.chips.len() >= 5, "{:?}", home.chips);
    assert!(home.chips.iter().all(|c| c.target.is_some()));
    assert!(home.continuation.is_some());
}

#[test]
fn explore() {
    let explore = page("explore");
    let buttons = explore
        .shelves
        .iter()
        .find(|s| s.style == ShelfStyle::Buttons && s.title.is_empty())
        .expect("the New releases / Charts / Moods & genres buttons");
    assert!(buttons.items.iter().all(|i| i.target.is_some()));
    let albums = shelf(&explore, "New albums & singles");
    for card in &albums.items {
        assert_card(card);
        assert_eq!(card.kind, ItemKind::Album);
        assert!(browse_id(&card.target).starts_with("MPRE"));
    }
    let moods = shelf(&explore, "Moods & genres");
    assert_eq!(moods.style, ShelfStyle::Buttons);
    for mood in &moods.items {
        assert!(
            matches!(
                &mood.target,
                Some(Target::Browse {
                    params: Some(_),
                    ..
                })
            ),
            "{:?}",
            mood.target
        );
    }
}

#[test]
fn search() {
    let search = page("search");
    let top = search.shelves.first().expect("shelves");
    assert_eq!(top.style, ShelfStyle::TopResult);
    let result = top.items.first().expect("a top result");
    assert_card(result);
    assert_eq!(result.kind, ItemKind::Artist);
    assert!(browse_id(&result.target).starts_with("UC"));
    let songs: Vec<&Track> = search
        .shelves
        .iter()
        .flat_map(|s| &s.items)
        .filter(|i| i.kind == ItemKind::Song)
        .map(assert_song)
        .collect();
    // The Songs section's rows give their length (other sections' may not).
    assert!(songs.first().expect("songs").duration.is_some());
    // The filter chips (Songs, Albums…) run the search again.
    assert!(search.chips.len() >= 3);
    assert!(
        search
            .chips
            .iter()
            .filter_map(|c| c.target.as_ref())
            .all(|t| matches!(t, Target::Search { .. }))
    );
}

#[test]
fn album() {
    let album = page("album");
    let header = album.header.as_ref().expect("a header");
    assert!(!header.title.is_empty());
    assert!(header.thumbnail.is_some());
    assert!(matches!(
        &header.play,
        Some(Target::Watch {
            playlist_id: Some(_),
            ..
        })
    ));
    assert!(header.shuffle.is_some() && header.radio.is_some());
    let tracks = ytfast::account::entries(&album).expect("the album's tracks");
    assert!(!tracks.items.is_empty());
    for row in &tracks.items {
        let track = row.track.as_ref().expect("a track");
        assert!(is_video_id(&track.video_id));
        assert!(track.duration.is_some(), "{:?}", track.title);
        // Album rows carry no cover of their own: they take the album's.
        assert_eq!(track.thumbnail, header.thumbnail);
    }
}

#[test]
fn artist() {
    let artist = page("artist");
    let header = artist.header.as_ref().expect("a header");
    assert!(!header.title.is_empty());
    assert!(header.radio.is_some());
    let subscription = header.subscription.as_ref().expect("a subscribe button");
    assert!(subscription.channel_id.starts_with("UC"));
    let songs = artist
        .shelves
        .iter()
        .find(|s| s.style == ShelfStyle::List)
        .expect("a songs list");
    assert!(!songs.items.is_empty());
    songs.items.iter().for_each(|i| {
        assert_song(i);
    });
    let albums = shelf(&artist, "Albums");
    for card in &albums.items {
        assert_card(card);
        assert_eq!(card.kind, ItemKind::Album);
        assert!(card.play.is_some(), "{:?} has no play button", card.title);
    }
}

#[test]
fn playlist() {
    let playlist = page("playlist");
    let header = playlist.header.as_ref().expect("a header");
    assert!(!header.title.is_empty());
    assert!(header.thumbnail.is_some());
    assert!(matches!(
        &header.play,
        Some(Target::Watch {
            playlist_id: Some(_),
            ..
        })
    ));
    let rows = ytfast::account::entries(&playlist).expect("the playlist's songs");
    assert!(!rows.items.is_empty());
    for row in &rows.items {
        let track = assert_song(row);
        assert!(track.duration.is_some(), "{:?}", track.title);
        assert!(track.thumbnail.is_some(), "{:?}", track.title);
    }
}

#[test]
fn mood() {
    let mood = page("mood");
    assert!(!mood.header.as_ref().expect("a header").title.is_empty());
    assert!(!mood.shelves.is_empty());
    for shelf in &mood.shelves {
        assert!(!shelf.title.is_empty());
        for card in &shelf.items {
            assert_card(card);
            if card.kind == ItemKind::Playlist {
                assert!(browse_id(&card.target).starts_with("VL"));
            }
        }
    }
}

#[test]
fn watch_next() {
    let next = parse::watch_next(&fixture("next"));
    let current = next.tracks.get(next.current).expect("the requested song");
    assert!(is_video_id(&current.video_id));
    assert!(!current.title.is_empty() && !current.artists.is_empty());
    assert!(current.duration.is_some());
    assert!(
        next.lyrics
            .as_deref()
            .is_some_and(|id| id.starts_with("MPLY"))
    );
    assert!(
        next.related
            .as_deref()
            .is_some_and(|id| id.starts_with("MPTR"))
    );
    assert!(next.radio.is_some(), "autoplay's radio");
    let (video_id, _) = next.like.as_ref().expect("the song's rating");
    assert_eq!(video_id, &current.video_id);
}

#[test]
fn lyrics() {
    let plain = parse::lyrics(&fixture("lyrics")).expect("plain lyrics");
    assert!(!plain.text.is_empty());
    assert!(plain.source.is_some());
    assert!(plain.lines.is_empty());

    let timed = ytfast::lyrics::youtube_timed(&fixture("lyrics_timed")).expect("timed lyrics");
    assert!(!timed.lines.is_empty());
    assert!(timed.lines.windows(2).all(|w| w[0].start <= w[1].start));
    assert!(timed.lines.iter().any(|l| !l.text.is_empty()));
}

/// Gives every other song row in `v` (`musicResponsiveListItemRenderer`
/// with a like button) the rating LIKE, the rest INDIFFERENT, as a
/// signed-in response would; returns each row's video id and rating.
fn rate_rows(v: &mut Value, out: &mut Vec<(String, LikeStatus)>) {
    match v {
        Value::Object(map) => {
            if let Some(row) = map.get_mut("musicResponsiveListItemRenderer") {
                let id = row
                    .pointer("/playlistItemData/videoId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                let button =
                    row.pointer_mut("/menu/menuRenderer/topLevelButtons/0/likeButtonRenderer");
                if let (Some(id), Some(Value::Object(button))) = (id, button) {
                    let status = if out.len().is_multiple_of(2) {
                        LikeStatus::Like
                    } else {
                        LikeStatus::Indifferent
                    };
                    let name = if status == LikeStatus::Like {
                        "LIKE"
                    } else {
                        "INDIFFERENT"
                    };
                    button.insert("likeStatus".into(), Value::from(name));
                    out.push((id, status));
                }
            }
            map.values_mut().for_each(|x| rate_rows(x, out));
        }
        Value::Array(items) => items.iter_mut().for_each(|x| rate_rows(x, out)),
        _ => {}
    }
}

/// Song rows carry the account's rating from their menu's like button:
/// INDIFFERENT signed out, and whatever a signed-in response says.
#[test]
fn row_ratings() {
    for name in ["playlist", "album", "search", "artist"] {
        let tracks = |page: &Page| -> Vec<Track> {
            page.shelves
                .iter()
                .flat_map(|s| &s.items)
                .filter_map(|i| i.track.clone())
                .collect()
        };
        let signed_out = tracks(&page(name));
        assert!(
            signed_out
                .iter()
                .any(|t| t.like == Some(LikeStatus::Indifferent)),
            "{name}: no rated rows"
        );
        assert!(signed_out.iter().all(|t| t.like != Some(LikeStatus::Like)));

        let mut v = fixture(name);
        let mut rated = Vec::new();
        rate_rows(&mut v, &mut rated);
        assert!(
            rated.len() >= 2,
            "{name}: {} rows with a like button",
            rated.len()
        );
        let parsed = tracks(&parse::page(&v));
        for (id, status) in &rated {
            if let Some(track) = parsed.iter().find(|t| &t.video_id == id) {
                assert_eq!(track.like, Some(*status), "{name}: {id}");
            }
        }
        assert!(
            parsed.iter().any(|t| t.like == Some(LikeStatus::Like)),
            "{name}: no liked row parsed"
        );
    }
}

/// Up next's rows: a hand-written `next` panel, one row liked, one with no
/// like button (as signed out).
#[test]
fn queue_ratings() {
    let row = |id: &str, like: Option<&str>| {
        let mut r = serde_json::json!({
            "videoId": id,
            "title": {"runs": [{"text": "Song"}]},
            "longBylineText": {"runs": [{"text": "Artist"}]},
            "lengthText": {"runs": [{"text": "3:21"}]},
        });
        if let Some(status) = like {
            r["menu"] = serde_json::json!({"menuRenderer": {"topLevelButtons": [
                {"likeButtonRenderer": {"target": {"videoId": id}, "likeStatus": status}}
            ]}});
        }
        serde_json::json!({"playlistPanelVideoRenderer": r})
    };
    let next = serde_json::json!({"contents": {"singleColumnMusicWatchNextResultsRenderer": {
        "tabbedRenderer": {"watchNextTabbedResultsRenderer": {"tabs": [{"tabRenderer": {
            "content": {"musicQueueRenderer": {"content": {"playlistPanelRenderer": {
                "contents": [row("aaaaaaaaaaa", Some("LIKE")), row("bbbbbbbbbbb", None)]
            }}}}
        }}]}}
    }}});
    let next = parse::watch_next(&next);
    let likes: Vec<_> = next.tracks.iter().map(|t| t.like).collect();
    assert_eq!(likes, [Some(LikeStatus::Like), None]);
}
