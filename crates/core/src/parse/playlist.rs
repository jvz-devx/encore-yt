//! Playlist-song pagination only. Page/filter reloads and radio continuations
//! are deliberately not accepted as the next page of a song list.

use serde_json::Value;

use crate::innertube::ApiError;
use crate::model::{PlaylistCounts, Track};

use super::items::item;
use super::value::{array, str_at};
use super::{find, text};

#[derive(Debug)]
pub(crate) struct Part {
    pub tracks: Vec<Track>,
    pub next: Option<String>,
    pub counts: PlaylistCounts,
}

fn song_shelf(value: &Value) -> Option<&Value> {
    match value {
        Value::Object(object) => {
            for key in ["musicPlaylistShelfRenderer", "musicShelfRenderer"] {
                if let Some(shelf) = object.get(key)
                    && text(shelf.get("title")).is_empty()
                    && shelf.get("contents").is_some_and(Value::is_array)
                {
                    return Some(shelf);
                }
            }
            object.values().find_map(song_shelf)
        }
        Value::Array(values) => values.iter().find_map(song_shelf),
        _ => None,
    }
}

fn expected_count(value: &Value) -> Option<usize> {
    let header = find(value, "musicResponsiveHeaderRenderer")?;
    let subtitle = text(header.get("secondSubtitle"));
    subtitle.split('•').find_map(|part| {
        let part = part.trim();
        let number = part
            .strip_suffix(" songs")
            .or_else(|| part.strip_suffix(" song"))?;
        let digits: String = number.chars().filter(|c| c.is_ascii_digit()).collect();
        digits.parse().ok()
    })
}

fn next(container: &Value, rows: &[Value]) -> Result<Option<String>, ApiError> {
    if let Some(token) = array(container.get("continuations"))
        .iter()
        .find_map(|value| str_at(value, &["nextContinuationData", "continuation"]))
    {
        return Ok(Some(token.into()));
    }
    for row in rows.iter().rev() {
        let Some(endpoint) = row
            .get("continuationItemRenderer")
            .and_then(|r| r.get("continuationEndpoint"))
        else {
            continue;
        };
        if let Some(command) = endpoint.get("continuationCommand") {
            let request = command.get("request").and_then(Value::as_str);
            if request.is_none() || request == Some("CONTINUATION_REQUEST_TYPE_BROWSE") {
                return command
                    .get("token")
                    .and_then(Value::as_str)
                    .map(|s| Some(s.into()))
                    .ok_or_else(|| ApiError::Invalid("playlist continuation has no token".into()));
            }
        }
        for command in array(
            endpoint
                .get("commandExecutorCommand")
                .and_then(|c| c.get("commands")),
        ) {
            if str_at(command, &["continuationCommand", "request"])
                == Some("CONTINUATION_REQUEST_TYPE_BROWSE")
            {
                return str_at(command, &["continuationCommand", "token"])
                    .map(|s| Some(s.into()))
                    .ok_or_else(|| {
                        ApiError::Invalid("playlist browse command has no token".into())
                    });
            }
        }
        return Err(ApiError::Invalid(
            "unrecognised playlist continuation command".into(),
        ));
    }
    Ok(None)
}

fn decode(container: &Value, rows: &[Value]) -> Result<Part, ApiError> {
    let mut tracks = Vec::new();
    let mut counts = PlaylistCounts::default();
    for row in rows {
        if row.get("continuationItemRenderer").is_some() {
            continue;
        }
        let Some(renderer) = row.get("musicResponsiveListItemRenderer") else {
            // A recognized empty terminal page is valid, but unfamiliar row
            // renderers must be counted rather than silently called songs.
            counts.rows += 1;
            counts.unparsed += 1;
            continue;
        };
        counts.rows += 1;
        let unavailable = str_at(renderer, &["musicItemRendererDisplayPolicy"])
            == Some("MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT")
            || text(
                renderer
                    .get("flexColumns")
                    .and_then(|columns| columns.get(0))
                    .and_then(|column| column.get("musicResponsiveListItemFlexColumnRenderer"))
                    .and_then(|column| column.get("text")),
            ) == "Song deleted";
        if unavailable {
            counts.unavailable += 1;
        } else if let Some(track) = item(row).and_then(|item| item.track) {
            tracks.push(track);
        } else {
            counts.unparsed += 1;
        }
    }
    Ok(Part {
        tracks,
        next: next(container, rows)?,
        counts,
    })
}

pub(crate) fn first(value: &Value) -> Result<Part, ApiError> {
    let shelf = value
        .get("contents")
        .and_then(song_shelf)
        .ok_or_else(|| ApiError::Invalid("playlist song shelf missing".into()))?;
    let mut part = decode(shelf, array(shelf.get("contents")))?;
    part.counts.expected = expected_count(value);
    // The normal page parser supplies album covers inherited by song rows.
    let page = super::page(value);
    if let Some(header) = page.header {
        for track in &mut part.tracks {
            if track.thumbnail.is_none() {
                track.thumbnail.clone_from(&header.thumbnail);
            }
        }
    }
    Ok(part)
}

pub(crate) fn more(value: &Value) -> Result<Part, ApiError> {
    if let Some(contents) = value.get("continuationContents") {
        for key in ["musicPlaylistShelfContinuation", "musicShelfContinuation"] {
            if let Some(shelf) = contents.get(key) {
                let rows = shelf
                    .get("contents")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        ApiError::Invalid("playlist continuation rows missing".into())
                    })?;
                return decode(shelf, rows);
            }
        }
        if let Some(section) = contents.get("sectionListContinuation") {
            let shelf = song_shelf(section).ok_or_else(|| {
                ApiError::Invalid("playlist continuation song shelf missing".into())
            })?;
            return decode(shelf, array(shelf.get("contents")));
        }
    }
    for key in [
        "onResponseReceivedActions",
        "onResponseReceivedEndpoints",
        "onResponseReceivedCommands",
    ] {
        let mut rows = Vec::new();
        let mut recognized = false;
        for action in array(value.get(key)) {
            if let Some(items) = action
                .get("appendContinuationItemsAction")
                .and_then(|a| a.get("continuationItems"))
                .and_then(Value::as_array)
            {
                recognized = true;
                rows.extend(items.iter().cloned());
            }
        }
        if recognized {
            return decode(&Value::Null, &rows);
        }
    }
    Err(ApiError::Invalid("playlist continuation missing".into()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "synthetic protocol fixture assertions")]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(id: &str) -> Value {
        json!({"musicResponsiveListItemRenderer": {
            "flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Synthetic song"}]}}}],
            "playlistItemData": {"videoId": id, "playlistSetVideoId": format!("entry-{id}")}
        }})
    }

    #[test]
    fn completed_song_shelf_ignores_section_and_filter_reload_tokens() {
        let value = json!({"contents": {"sectionListRenderer": {
            "contents": [{"musicPlaylistShelfRenderer": {"contents": [row("one")]}}],
            "continuations": [{"reloadContinuationData": {"continuation": "not-more-songs"}}]
        }}});
        let got = first(&value).unwrap();
        assert_eq!(got.tracks.len(), 1);
        assert!(got.next.is_none());
    }

    #[test]
    fn nested_browse_commands_are_followed_but_radio_and_refresh_are_not() {
        let continuation = json!({"continuationItemRenderer": {"continuationEndpoint": {
            "commandExecutorCommand": {"commands": [
                {"playlistVotingRefreshPopupCommand": {}},
                {"continuationCommand": {"request": "CONTINUATION_REQUEST_TYPE_BROWSE", "token": "next-songs"}}
            ]}
        }}});
        let value = json!({"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [row("one"), continuation]}}]});
        assert_eq!(more(&value).unwrap().next.as_deref(), Some("next-songs"));
        let wrong = json!({"continuationContents": {"musicShelfContinuation": {
            "contents": [row("one"), {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"request": "CONTINUATION_REQUEST_TYPE_WATCH_NEXT", "token": "radio"}}}}]
        }}});
        assert!(more(&wrong).is_err());
    }

    #[test]
    fn counts_explain_unavailable_and_unparsed_rows_without_inventing_missing_songs() {
        let mut unavailable = row("unavailable");
        unavailable["musicResponsiveListItemRenderer"]["musicItemRendererDisplayPolicy"] =
            json!("MUSIC_ITEM_RENDERER_DISPLAY_POLICY_GREY_OUT");
        let value = json!({"header": {"musicResponsiveHeaderRenderer": {"secondSubtitle": {"runs": [{"text": "1,282 songs • 5+ hours"}]}}},
            "contents": {"musicPlaylistShelfRenderer": {"contents": [row("one"), unavailable, {"musicResponsiveListItemRenderer": {}}]}}});
        let got = first(&value).unwrap();
        assert_eq!(got.tracks.len(), 1);
        assert_eq!(got.counts.rows, 3);
        assert_eq!(got.counts.unavailable, 1);
        assert_eq!(got.counts.unparsed, 1);
        assert_eq!(got.counts.expected, Some(1282));
    }

    #[test]
    fn supported_continuation_envelopes_keep_their_song_shelf_ownership() {
        for key in ["musicShelfContinuation", "musicPlaylistShelfContinuation"] {
            let value = json!({"continuationContents": {key: {"contents": [row("one")], "continuations": [{"nextContinuationData": {"continuation": "next"}}]}}});
            assert_eq!(more(&value).unwrap().next.as_deref(), Some("next"));
        }
        let section = json!({"continuationContents": {"sectionListContinuation": {"contents": [
            {"musicShelfRenderer": {"title": {"runs": [{"text": "Suggestions"}]}, "contents": [row("wrong")]}},
            {"musicPlaylistShelfRenderer": {"contents": [row("one")]}}
        ]}}});
        let got = more(&section).unwrap();
        assert_eq!(got.tracks.len(), 1);
        assert_eq!(got.tracks[0].video_id, "one");
        for key in [
            "onResponseReceivedActions",
            "onResponseReceivedEndpoints",
            "onResponseReceivedCommands",
        ] {
            assert!(
                more(&json!({key: [{"appendContinuationItemsAction": {"continuationItems": []}}]}))
                    .unwrap()
                    .next
                    .is_none()
            );
        }
        assert!(more(&json!({"error": {"message": "synthetic failure"}})).is_err());
    }
}
