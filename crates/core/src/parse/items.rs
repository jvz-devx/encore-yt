//! InnerTube cards, song rows, navigation targets and row ratings.

use super::value::{array, at, find, str_at, text};
use crate::model::{Item, ItemKind, LikeStatus, Run, Target, Track, parse_duration};
use serde_json::Value;

pub(super) fn runs(v: Option<&Value>) -> Vec<Run> {
    let Some(v) = v else { return Vec::new() };
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return vec![Run {
            text: s.to_owned(),
            target: None,
        }];
    }
    array(v.get("runs"))
        .iter()
        .filter_map(|r| {
            Some(Run {
                text: r.get("text")?.as_str()?.to_owned(),
                target: r.get("navigationEndpoint").and_then(endpoint),
            })
        })
        .collect()
}

/// The action an endpoint performs.
pub fn endpoint(ep: &Value) -> Option<Target> {
    let params = |v: &Value| {
        v.get("params")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    if let Some(b) = ep.get("browseEndpoint") {
        return Some(Target::Browse {
            id: b.get("browseId")?.as_str()?.to_owned(),
            params: params(b),
        });
    }
    if let Some(w) = ep.get("watchEndpoint") {
        return Some(Target::Watch {
            video_id: w.get("videoId").and_then(Value::as_str).map(str::to_owned),
            playlist_id: w
                .get("playlistId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            params: params(w),
        });
    }
    if let Some(w) = ep.get("watchPlaylistEndpoint") {
        return Some(Target::Watch {
            video_id: None,
            playlist_id: Some(w.get("playlistId")?.as_str()?.to_owned()),
            params: params(w),
        });
    }
    if let Some(s) = ep.get("searchEndpoint") {
        return Some(Target::Search {
            query: s.get("query")?.as_str()?.to_owned(),
            params: params(s),
        });
    }
    None
}

fn page_type(ep: &Value) -> Option<&str> {
    str_at(
        ep,
        &[
            "browseEndpoint",
            "browseEndpointContextSupportedConfigs",
            "browseEndpointContextMusicConfig",
            "pageType",
        ],
    )
}

fn video_type(ep: &Value) -> Option<&str> {
    str_at(
        ep,
        &[
            "watchEndpoint",
            "watchEndpointMusicSupportedConfigs",
            "watchEndpointMusicConfig",
            "musicVideoType",
        ],
    )
}

pub(super) fn kind_of(ep: Option<&Value>) -> ItemKind {
    let Some(ep) = ep else { return ItemKind::Other };
    if ep.get("watchEndpoint").is_some() {
        return match video_type(ep) {
            Some("MUSIC_VIDEO_TYPE_ATV") | None => ItemKind::Song,
            _ => ItemKind::Video,
        };
    }
    if ep.get("watchPlaylistEndpoint").is_some() {
        return ItemKind::Playlist;
    }
    match page_type(ep) {
        Some("MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK") => ItemKind::Album,
        Some(
            "MUSIC_PAGE_TYPE_ARTIST"
            | "MUSIC_PAGE_TYPE_USER_CHANNEL"
            | "MUSIC_PAGE_TYPE_LIBRARY_ARTIST",
        ) => ItemKind::Artist,
        Some("MUSIC_PAGE_TYPE_PLAYLIST") => ItemKind::Playlist,
        _ => ItemKind::Other,
    }
}

/// The largest thumbnail under `v`, asking Google's image server for a
/// size that stays sharp on a large cover.
pub fn thumbnail(v: Option<&Value>) -> Option<String> {
    let list = find(v?, "thumbnails")?.as_array()?;
    let best = list
        .iter()
        .max_by_key(|t| t.get("width").and_then(Value::as_u64).unwrap_or(0))?;
    let url = best.get("url")?.as_str()?;
    let url = if url.starts_with("//") {
        format!("https:{url}")
    } else {
        url.to_owned()
    };
    Some(upscale(&url))
}

fn upscale(url: &str) -> String {
    if (url.contains("googleusercontent.com") || url.contains("ggpht.com"))
        && let Some(eq) = url.rfind('=')
        && (url[eq..].contains("-h") || url[eq..].starts_with("=w") || url[eq..].starts_with("=s"))
    {
        return format!("{}=w544-h544-l90-rj", &url[..eq]);
    }
    url.to_owned()
}

pub(super) fn play_button(r: &Value) -> Option<Target> {
    let overlay = r.get("thumbnailOverlay").or_else(|| r.get("overlay"))?;
    endpoint(at(
        overlay,
        &[
            "musicItemThumbnailOverlayRenderer",
            "content",
            "musicPlayButtonRenderer",
            "playNavigationEndpoint",
        ],
    )?)
}

pub(super) fn is_artist_id(t: &Option<Target>) -> bool {
    matches!(t, Some(Target::Browse { id, .. }) if id.starts_with("UC"))
}

fn is_album_id(t: &Option<Target>) -> bool {
    matches!(t, Some(Target::Browse { id, .. }) if id.starts_with("MPRE"))
}

/// Artist and album runs from a song's subtitle. Without artist links, the
/// first " • " segment that is not the item type is the artist.
fn artists_and_album(subtitle: &[Run]) -> (Vec<Run>, Option<Run>) {
    let album = subtitle.iter().find(|r| is_album_id(&r.target)).cloned();
    let linked: Vec<usize> = subtitle
        .iter()
        .enumerate()
        .filter(|(_, r)| is_artist_id(&r.target))
        .map(|(i, _)| i)
        .collect();
    if let (Some(&first), Some(&last)) = (linked.first(), linked.last()) {
        return (subtitle[first..=last].to_vec(), album);
    }
    let joined: String = subtitle.iter().map(|r| r.text.as_str()).collect();
    let artist = joined
        .split(" • ")
        .map(str::trim)
        .find(|s| {
            !s.is_empty() && !matches!(*s, "Song" | "Video" | "Single" | "EP" | "Album" | "Episode")
        })
        .unwrap_or("")
        .to_owned();
    (
        if artist.is_empty() {
            Vec::new()
        } else {
            vec![Run {
                text: artist,
                target: None,
            }]
        },
        album,
    )
}

fn duration_in(subtitle: &[Run]) -> Option<u32> {
    subtitle.iter().rev().find_map(|r| {
        let t = r.text.trim();
        (t.contains(':') && t.chars().all(|c| c.is_ascii_digit() || c == ':'))
            .then(|| parse_duration(t))
            .flatten()
    })
}

pub(super) fn track_from(
    video_id: &str,
    title: &str,
    subtitle: &[Run],
    thumb: Option<String>,
    duration: Option<u32>,
) -> Track {
    let (artists, album) = artists_and_album(subtitle);
    Track {
        video_id: video_id.to_owned(),
        title: title.to_owned(),
        artists,
        album,
        thumbnail: thumb,
        duration: duration.or_else(|| duration_in(subtitle)),
        like: None,
        set_video_id: None,
    }
}

/// `musicTwoRowItemRenderer`: a card.
fn two_row(r: &Value) -> Option<Item> {
    let title = text(r.get("title"));
    let nav = r.get("navigationEndpoint");
    let target = nav.and_then(endpoint);
    let kind = kind_of(nav);
    let subtitle = runs(r.get("subtitle"));
    let thumb = thumbnail(r.get("thumbnailRenderer"));
    let track = match &target {
        Some(Target::Watch {
            video_id: Some(id), ..
        }) => Some(track_from(id, &title, &subtitle, thumb.clone(), None)),
        _ => None,
    };
    Some(Item {
        kind,
        title,
        subtitle,
        thumbnail: thumb,
        target,
        play: play_button(r),
        track,
        index: None,
        stripe: None,
        editable: r
            .get("menu")
            .and_then(|m| find(m, "playlistEditorEndpoint"))
            .and_then(|e| str_at(e, &["playlistId"]))
            .map(str::to_owned),
    })
}

/// `musicResponsiveListItemRenderer`: a row.
fn list_row(r: &Value) -> Option<Item> {
    let columns: Vec<&Value> = array(r.get("flexColumns"))
        .iter()
        .filter_map(|c| at(c, &["musicResponsiveListItemFlexColumnRenderer", "text"]))
        .collect();
    let title_value = columns.first().copied();
    let title = text(title_value);
    if title.is_empty() {
        return None;
    }
    let title_nav = title_value.and_then(|t| at(t, &["runs", "0", "navigationEndpoint"]));
    let mut subtitle = Vec::new();
    for column in columns.iter().skip(1) {
        let mut column_runs = runs(Some(column));
        if column_runs.is_empty() {
            continue;
        }
        if !subtitle.is_empty() {
            subtitle.push(Run {
                text: " • ".into(),
                target: None,
            });
        }
        subtitle.append(&mut column_runs);
    }
    let fixed = array(r.get("fixedColumns"))
        .first()
        .map(|c| {
            text(at(
                c,
                &["musicResponsiveListItemFixedColumnRenderer", "text"],
            ))
        })
        .filter(|s| !s.is_empty());
    let thumb = thumbnail(r.get("thumbnail"));
    let play = play_button(r);
    let video_id = str_at(r, &["playlistItemData", "videoId"])
        .or_else(|| title_nav.and_then(|n| str_at(n, &["watchEndpoint", "videoId"])))
        .map(str::to_owned)
        .or_else(|| match &play {
            Some(Target::Watch {
                video_id: Some(id), ..
            }) => Some(id.clone()),
            _ => None,
        });
    let greyed = str_at(r, &["musicItemRendererDisplayPolicy"])
        == Some("MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT");
    let index = text(r.get("index")).trim().to_owned();
    let index = (!index.is_empty()).then_some(index);
    let own_nav = r.get("navigationEndpoint");
    if let (Some(id), false) = (&video_id, greyed) {
        let kind = match title_nav.and_then(video_type).or_else(|| {
            r.get("overlay")
                .and_then(|o| find(o, "musicVideoType"))
                .and_then(Value::as_str)
        }) {
            Some("MUSIC_VIDEO_TYPE_ATV") | None => ItemKind::Song,
            _ => ItemKind::Video,
        };
        let duration = fixed.as_deref().and_then(parse_duration);
        let mut track = track_from(id, &title, &subtitle, thumb.clone(), duration);
        track.like = row_like(r);
        // A playlist's suggested songs carry the placeholder entry id
        // `to_be_updated_by_client` (per ytmusicapi); only real entries can be edited.
        track.set_video_id = str_at(r, &["playlistItemData", "playlistSetVideoId"])
            .filter(|s| *s != "to_be_updated_by_client")
            .map(str::to_owned);
        return Some(Item {
            kind,
            title,
            subtitle,
            thumbnail: thumb,
            target: Some(Target::Watch {
                video_id: Some(id.clone()),
                playlist_id: None,
                params: None,
            }),
            play,
            track: Some(track),
            index,
            stripe: None,
            editable: None,
        });
    }
    let nav = own_nav.or(title_nav);
    Some(Item {
        kind: kind_of(nav),
        title,
        subtitle,
        thumbnail: thumb,
        target: nav.and_then(endpoint),
        play,
        track: None,
        index,
        stripe: None,
        editable: None,
    })
}

/// `musicNavigationButtonRenderer`: a mood/genre or Explore button.
fn nav_button(r: &Value) -> Option<Item> {
    Some(Item {
        kind: ItemKind::Button,
        title: text(r.get("buttonText")),
        subtitle: Vec::new(),
        thumbnail: None,
        target: r.get("clickCommand").and_then(endpoint),
        play: None,
        track: None,
        index: None,
        stripe: at(r, &["solid", "leftStripeColor"])
            .and_then(Value::as_u64)
            .and_then(|c| u32::try_from(c).ok()),
        editable: None,
    })
}

pub(super) fn item(v: &Value) -> Option<Item> {
    if let Some(r) = v.get("musicTwoRowItemRenderer") {
        return two_row(r);
    }
    if let Some(r) = v.get("musicResponsiveListItemRenderer") {
        return list_row(r);
    }
    if let Some(r) = v.get("musicNavigationButtonRenderer") {
        return nav_button(r);
    }
    None
}

pub(super) fn like_status(v: Option<&Value>) -> Option<LikeStatus> {
    match v?.as_str()? {
        "LIKE" => Some(LikeStatus::Like),
        "DISLIKE" => Some(LikeStatus::Dislike),
        "INDIFFERENT" => Some(LikeStatus::Indifferent),
        _ => None,
    }
}

/// A row's rating, from the like button in its menu (list rows and Up
/// next's rows) or, where a row has no like button (most search results),
/// from its menu's "Add to liked songs" toggle; `None` where the response
/// has neither.
pub(super) fn row_like(r: &Value) -> Option<LikeStatus> {
    let menu = at(r, &["menu", "menuRenderer"]);
    array(menu.and_then(|m| m.get("topLevelButtons")))
        .iter()
        .find_map(|b| like_status(at(b, &["likeButtonRenderer", "likeStatus"])))
        .or_else(|| {
            array(menu.and_then(|m| m.get("items")))
                .iter()
                .find_map(liked_songs_toggle)
        })
}

/// The rating a menu's liked-songs toggle implies: its first action is
/// what a click does, a like when the song isn't liked and "Remove from
/// liked songs" (`INDIFFERENT`) when it is. A disliked song offers the like
/// as well, so it reads as not liked.
fn liked_songs_toggle(item: &Value) -> Option<LikeStatus> {
    let like = at(
        item,
        &[
            "toggleMenuServiceItemRenderer",
            "defaultServiceEndpoint",
            "likeEndpoint",
        ],
    )?;
    str_at(like, &["target", "videoId"])?;
    match like_status(like.get("status"))? {
        LikeStatus::Indifferent => Some(LikeStatus::Like),
        LikeStatus::Like => Some(LikeStatus::Indifferent),
        LikeStatus::Dislike => None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests inspect synthetic navigation renderers"
)]
mod tests {
    use super::*;

    #[test]
    fn oversized_stripe_colours_are_omitted_instead_of_wrapping() {
        let button = serde_json::json!({"solid": {"leftStripeColor": u64::MAX}});
        assert_eq!(nav_button(&button).unwrap().stripe, None);
        let button = serde_json::json!({"solid": {"leftStripeColor": u32::MAX}});
        assert_eq!(nav_button(&button).unwrap().stripe, Some(u32::MAX));
    }
}
