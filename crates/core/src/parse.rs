//! Translates YouTube Music's InnerTube JSON into [`crate::model`] types.
//!
//! YouTube Music builds every page from a few renderer families: two-row
//! cards, responsive list rows, carousel/list/grid shelves and a handful of
//! headers. Parsing is by family, not by page, so Home, Explore, Library,
//! albums, artists, playlists and search share one path.

mod items;
mod value;

pub use items::{endpoint, thumbnail};
use items::{is_artist_id, item, kind_of, like_status, play_button, row_like, runs, track_from};
use value::{array, str_at};
pub use value::{at, find, text};

use serde_json::Value;

use crate::model::{
    Chip, Header, Item, ItemKind, Lyrics, Page, Run, Shelf, ShelfStyle, Target, Track, WatchNext,
    parse_duration,
};

/// A continuation token from the old (`continuations`) or new
/// (`continuationItemRenderer`) form.
fn continuation(container: &Value, items: &[Value]) -> Option<String> {
    if let Some(token) = array(container.get("continuations")).iter().find_map(|c| {
        c.get("nextContinuationData")
            .or_else(|| c.get("reloadContinuationData"))
            .or_else(|| c.get("nextRadioContinuationData"))
            .and_then(|d| d.get("continuation"))
            .and_then(Value::as_str)
    }) {
        return Some(token.to_owned());
    }
    items.iter().rev().find_map(|i| {
        str_at(
            i,
            &[
                "continuationItemRenderer",
                "continuationEndpoint",
                "continuationCommand",
                "token",
            ],
        )
        .map(str::to_owned)
    })
}

/// Items that do something; YouTube Music also lists inert tiles ("New playlist").
fn items_of(list: &[Value]) -> Vec<Item> {
    list.iter()
        .filter_map(item)
        .filter(|i| i.target.is_some() || i.play.is_some() || i.track.is_some())
        .collect()
}

/// One section of a section list, as zero or more shelves.
fn section(v: &Value, page: &mut Page) -> Vec<Shelf> {
    if let Some(r) = v
        .get("musicCarouselShelfRenderer")
        .or_else(|| v.get("musicImmersiveCarouselShelfRenderer"))
    {
        let header = r.get("header").and_then(|h| {
            h.get("musicCarouselShelfBasicHeaderRenderer")
                .or_else(|| h.get("musicImmersiveCarouselShelfBasicHeaderRenderer"))
        });
        let raw = array(r.get("contents"));
        let rows_raw = raw
            .iter()
            .any(|i| i.get("musicResponsiveListItemRenderer").is_some());
        let items = items_of(raw);
        if items.is_empty() {
            return Vec::new();
        }
        let style = if rows_raw {
            ShelfStyle::RowCarousel
        } else if items.iter().all(|i| i.kind == ItemKind::Button) {
            ShelfStyle::Buttons
        } else {
            ShelfStyle::Carousel
        };
        return vec![Shelf {
            title: text(header.and_then(|h| h.get("title"))),
            strapline: header
                .map(|h| text(h.get("strapline")))
                .filter(|s| !s.is_empty()),
            style,
            items,
            more: header
                .and_then(|h| {
                    at(
                        h,
                        &["moreContentButton", "buttonRenderer", "navigationEndpoint"],
                    )
                })
                .and_then(endpoint),
            continuation: None,
        }];
    }
    if let Some(r) = v
        .get("musicShelfRenderer")
        .or_else(|| v.get("musicPlaylistShelfRenderer"))
    {
        let raw = array(r.get("contents"));
        let items = items_of(raw);
        let more = r.get("bottomEndpoint").and_then(endpoint);
        if items.is_empty() && more.is_none() {
            return Vec::new();
        }
        return vec![Shelf {
            title: text(r.get("title")),
            strapline: None,
            style: ShelfStyle::List,
            items,
            more,
            continuation: continuation(r, raw),
        }];
    }
    if let Some(r) = v.get("gridRenderer") {
        let raw = array(r.get("items"));
        let items = items_of(raw);
        if items.is_empty() {
            return Vec::new();
        }
        let style = if items.iter().all(|i| i.kind == ItemKind::Button) {
            ShelfStyle::Buttons
        } else {
            ShelfStyle::Grid
        };
        return vec![Shelf {
            title: text(at(r, &["header", "gridHeaderRenderer", "title"])),
            strapline: None,
            style,
            items,
            more: None,
            continuation: continuation(r, raw),
        }];
    }
    if let Some(r) = v.get("musicCardShelfRenderer") {
        let mut shelves = Vec::new();
        let title_value = r.get("title");
        let nav = title_value.and_then(|t| at(t, &["runs", "0", "navigationEndpoint"]));
        let title = text(title_value);
        let subtitle = runs(r.get("subtitle"));
        let thumb = thumbnail(r.get("thumbnail"));
        let target = nav.and_then(endpoint);
        let track = match &target {
            Some(Target::Watch {
                video_id: Some(id), ..
            }) => Some(track_from(id, &title, &subtitle, thumb.clone(), None)),
            _ => None,
        };
        let play = array(r.get("buttons"))
            .iter()
            .find_map(|b| {
                b.get("buttonRenderer")
                    .and_then(|b| b.get("command"))
                    .and_then(endpoint)
            })
            .or_else(|| play_button(r));
        shelves.push(Shelf {
            title: text(r.get("header").and_then(|h| find(h, "title"))),
            strapline: None,
            style: ShelfStyle::TopResult,
            items: vec![Item {
                kind: kind_of(nav),
                title,
                subtitle,
                thumbnail: thumb,
                target,
                play,
                track,
                index: None,
                stripe: None,
                editable: None,
            }],
            more: None,
            continuation: None,
        });
        let rows = items_of(array(r.get("contents")));
        if !rows.is_empty() {
            shelves.push(Shelf {
                title: String::new(),
                strapline: None,
                style: ShelfStyle::List,
                items: rows,
                more: None,
                continuation: None,
            });
        }
        return shelves;
    }
    if let Some(r) = v.get("itemSectionRenderer") {
        let mut shelves = Vec::new();
        for inner in array(r.get("contents")) {
            if let Some(m) = inner.get("messageRenderer") {
                let message = [
                    text(m.get("text")),
                    text(at(m, &["subtext", "messageSubtextRenderer", "text"])),
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
                if !message.is_empty() {
                    page.message = Some(message);
                }
            } else if let Some(i) = item(inner) {
                shelves.push(i);
            }
        }
        return if shelves.is_empty() {
            Vec::new()
        } else {
            vec![Shelf {
                title: String::new(),
                strapline: None,
                style: ShelfStyle::List,
                items: shelves,
                more: None,
                continuation: None,
            }]
        };
    }
    if let Some(r) = v.get("musicDescriptionShelfRenderer") {
        if let Some(header) = page.header.as_mut()
            && header.description.is_none()
        {
            header.description = Some(text(r.get("description"))).filter(|s| !s.is_empty());
        }
        return Vec::new();
    }
    if let Some(m) = v.get("messageRenderer") {
        page.message = Some(text(m.get("text"))).filter(|s| !s.is_empty());
    }
    Vec::new()
}

fn chips(v: Option<&Value>) -> Vec<Chip> {
    let Some(v) = v else { return Vec::new() };
    let Some(cloud) = find(v, "chipCloudRenderer") else {
        return Vec::new();
    };
    array(cloud.get("chips"))
        .iter()
        .filter_map(|c| {
            let c = c.get("chipCloudChipRenderer")?;
            Some(Chip {
                text: text(c.get("text")),
                target: c.get("navigationEndpoint").and_then(endpoint),
                selected: c
                    .get("isSelected")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                deselect: c.get("onDeselectedCommand").and_then(endpoint),
                reload: str_at(
                    c,
                    &[
                        "navigationEndpoint",
                        "browseSectionListReloadEndpoint",
                        "continuation",
                        "reloadContinuationData",
                        "continuation",
                    ],
                )
                .map(str::to_owned),
            })
        })
        .filter(|c| !c.text.is_empty())
        .collect()
}

fn menu_target(r: &Value, icon: &str) -> Option<Target> {
    let menu = find(r, "menuRenderer")?;
    array(menu.get("items")).iter().find_map(|i| {
        let n = i.get("menuNavigationItemRenderer")?;
        (str_at(n, &["icon", "iconType"]) == Some(icon))
            .then(|| n.get("navigationEndpoint").and_then(endpoint))
            .flatten()
    })
}

fn header(v: &Value) -> Option<Header> {
    if let Some(r) = v.get("musicResponsiveHeaderRenderer") {
        let mut subtitle = runs(r.get("straplineTextOne"));
        let second = runs(r.get("subtitle"));
        if !subtitle.is_empty() && !second.is_empty() {
            subtitle.push(Run {
                text: " • ".into(),
                target: None,
            });
        }
        subtitle.extend(second);
        let play = array(r.get("buttons")).iter().find_map(|b| {
            b.get("musicPlayButtonRenderer")
                .and_then(|p| p.get("playNavigationEndpoint"))
                .and_then(endpoint)
        });
        return Some(Header {
            title: text(r.get("title")),
            subtitle,
            second_subtitle: text(r.get("secondSubtitle")),
            description: r
                .get("description")
                .map(|d| text(find(d, "description")))
                .filter(|s| !s.is_empty()),
            thumbnail: thumbnail(r.get("thumbnail")),
            round: false,
            play,
            shuffle: menu_target(r, "MUSIC_SHUFFLE"),
            radio: menu_target(r, "MIX"),
            ..Header::default()
        });
    }
    if let Some(r) = v.get("musicEditablePlaylistDetailHeaderRenderer") {
        return r.get("header").and_then(header);
    }
    if let Some(r) = v
        .get("musicImmersiveHeaderRenderer")
        .or_else(|| v.get("musicVisualHeaderRenderer"))
    {
        return Some(Header {
            title: text(r.get("title")),
            subtitle: runs(
                r.get("subscriptionButton")
                    .and_then(|s| find(s, "longSubscriberCountText"))
                    .or(r.get("subtitle")),
            ),
            second_subtitle: String::new(),
            description: Some(text(r.get("description"))).filter(|s| !s.is_empty()),
            thumbnail: thumbnail(r.get("foregroundThumbnail").or(r.get("thumbnail"))),
            round: true,
            play: at(r, &["playButton", "buttonRenderer", "navigationEndpoint"]).and_then(endpoint),
            shuffle: None,
            radio: at(
                r,
                &["startRadioButton", "buttonRenderer", "navigationEndpoint"],
            )
            .and_then(endpoint),
            ..Header::default()
        });
    }
    if let Some(r) = v.get("musicHeaderRenderer") {
        let title = text(r.get("title"));
        return (!title.is_empty()).then(|| Header {
            title,
            ..Header::default()
        });
    }
    if let Some(r) = v.get("musicDetailHeaderRenderer") {
        return Some(Header {
            title: text(r.get("title")),
            subtitle: runs(r.get("subtitle")),
            second_subtitle: text(r.get("secondSubtitle")),
            description: Some(text(r.get("description"))).filter(|s| !s.is_empty()),
            thumbnail: thumbnail(r.get("thumbnail")),
            ..Header::default()
        });
    }
    None
}

fn section_list(list: &Value, page: &mut Page) {
    for s in array(list.get("contents")) {
        if page.header.is_none()
            && let Some(h) = header(s)
        {
            page.header = Some(h);
            continue;
        }
        for shelf in section(s, page) {
            // Search lists results as one-row sections; keep them as one list.
            let untitled_list = |s: &Shelf| {
                s.style == ShelfStyle::List
                    && s.title.is_empty()
                    && s.more.is_none()
                    && s.continuation.is_none()
            };
            match page.shelves.last_mut() {
                Some(last) if untitled_list(last) && untitled_list(&shelf) => {
                    last.items.extend(shelf.items)
                }
                _ => page.shelves.push(shelf),
            }
        }
    }
    if page.chips.is_empty() {
        page.chips = chips(list.get("header"));
    }
    if page.continuation.is_none() {
        page.continuation = continuation(list, array(list.get("contents")));
    }
}

/// A browse response (Home, Explore, Library, album, artist, playlist,
/// lyrics-less related pages…) or a search response.
pub fn page(v: &Value) -> Page {
    let mut page = Page {
        header: v.get("header").and_then(header),
        ..Page::default()
    };
    let contents = v.get("contents");
    if let Some(single) = contents.and_then(|c| {
        c.get("singleColumnBrowseResultsRenderer")
            .or_else(|| c.get("tabbedSearchResultsRenderer"))
    }) {
        if let Some(list) = at(
            single,
            &["tabs", "0", "tabRenderer", "content", "sectionListRenderer"],
        ) {
            section_list(list, &mut page);
        }
    } else if let Some(two) = contents.and_then(|c| c.get("twoColumnBrowseResultsRenderer")) {
        if let Some(list) = at(
            two,
            &["tabs", "0", "tabRenderer", "content", "sectionListRenderer"],
        ) {
            section_list(list, &mut page);
        }
        if let Some(list) = at(two, &["secondaryContents", "sectionListRenderer"]) {
            section_list(list, &mut page);
        }
    } else if let Some(list) = contents.and_then(|c| c.get("sectionListRenderer")) {
        section_list(list, &mut page);
    }
    account_header(v, &mut page);
    fill_album_tracks(&mut page);
    page
}

/// Album rows carry neither cover nor album name; take them from the header.
fn fill_album_tracks(page: &mut Page) {
    let Some(h) = &page.header else { return };
    let album = Run {
        text: h.title.clone(),
        target: None,
    };
    let artists: Vec<Run> = h
        .subtitle
        .iter()
        .filter(|r| is_artist_id(&r.target))
        .cloned()
        .collect();
    for shelf in &mut page.shelves {
        for item in &mut shelf.items {
            if let Some(track) = &mut item.track
                && track.thumbnail.is_none()
            {
                track.thumbnail = h.thumbnail.clone();
                if track.album.is_none() {
                    track.album = Some(album.clone());
                }
                if track.artists.is_empty() {
                    track.artists = artists.clone();
                }
            }
        }
    }
}

/// What a continuation request returned.
pub enum More {
    /// Further shelves for the page (Home).
    Shelves {
        shelves: Vec<Shelf>,
        next: Option<String>,
    },
    /// Further rows or cards for the shelf that asked.
    Items {
        items: Vec<Item>,
        next: Option<String>,
    },
}

pub fn more(v: &Value) -> More {
    if let Some(cc) = v.get("continuationContents") {
        if let Some(list) = cc.get("sectionListContinuation") {
            let mut page = Page::default();
            section_list(list, &mut page);
            return More::Shelves {
                shelves: page.shelves,
                next: page.continuation,
            };
        }
        for key in [
            "musicShelfContinuation",
            "musicPlaylistShelfContinuation",
            "gridContinuation",
        ] {
            if let Some(s) = cc.get(key) {
                let raw = array(s.get("contents").or_else(|| s.get("items")));
                return More::Items {
                    items: items_of(raw),
                    next: continuation(s, raw),
                };
            }
        }
    }
    let raw = array(find(v, "continuationItems"));
    More::Items {
        items: items_of(raw),
        next: continuation(&Value::Null, raw),
    }
}

pub fn suggestions(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for section in array(v.get("contents")) {
        for s in array(at(
            section,
            &["searchSuggestionsSectionRenderer", "contents"],
        )) {
            let r = s
                .get("searchSuggestionRenderer")
                .or_else(|| s.get("historySuggestionRenderer"));
            if let Some(r) = r {
                let t = text(r.get("suggestion"));
                if !t.is_empty() && !out.contains(&t) {
                    out.push(t);
                }
            }
        }
    }
    out
}

fn panel_track(v: &Value) -> Option<(Track, bool)> {
    let r = v.get("playlistPanelVideoRenderer").or_else(|| {
        at(
            v,
            &[
                "playlistPanelVideoWrapperRenderer",
                "primaryRenderer",
                "playlistPanelVideoRenderer",
            ],
        )
    })?;
    let id = r.get("videoId")?.as_str()?;
    if r.get("unplayableText").is_some() {
        return None;
    }
    let byline = runs(r.get("longBylineText"));
    let duration = parse_duration(&text(r.get("lengthText")));
    let mut track = track_from(
        id,
        &text(r.get("title")),
        &byline,
        thumbnail(r.get("thumbnail")),
        duration,
    );
    // Signed in, Up next's rows carry the same like button as list rows.
    track.like = row_like(r);
    Some((
        track,
        r.get("selected").and_then(Value::as_bool).unwrap_or(false),
    ))
}

pub fn watch_next(v: &Value) -> WatchNext {
    let mut out = WatchNext::default();
    let tabs = array(find(v, "watchNextTabbedResultsRenderer").and_then(|w| w.get("tabs")));
    let panel = v
        .get("continuationContents")
        .and_then(|c| c.get("playlistPanelContinuation"))
        .or_else(|| tabs.first().and_then(|t| find(t, "playlistPanelRenderer")));
    if let Some(panel) = panel {
        let raw = array(panel.get("contents"));
        for entry in raw {
            if let Some((track, selected)) = panel_track(entry) {
                if selected {
                    out.current = out.tracks.len();
                }
                out.tracks.push(track);
            } else if let Some(auto) = find(entry, "automixPlaylistVideoRenderer") {
                out.radio = auto.get("navigationEndpoint").and_then(endpoint);
            }
        }
        out.continuation = continuation(panel, raw);
    }
    // Tabs are found by what they open: lyrics (MPLYt…) and related (MPTRt…);
    // their positions vary.
    for tab in tabs.iter().filter_map(|t| t.get("tabRenderer")) {
        if tab.get("unselectable").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        match str_at(tab, &["endpoint", "browseEndpoint", "browseId"]) {
            Some(id) if id.starts_with("MPLY") => out.lyrics = Some(id.to_owned()),
            Some(id) if id.starts_with("MPTR") => out.related = Some(id.to_owned()),
            _ => {}
        }
    }
    out.like = watch_like(v);
    out
}

pub fn lyrics(v: &Value) -> Option<Lyrics> {
    let shelf = find(v, "musicDescriptionShelfRenderer")?;
    let body = text(shelf.get("description"));
    (!body.is_empty()).then(|| Lyrics {
        text: body,
        source: Some(text(shelf.get("footer"))).filter(|s| !s.is_empty()),
        lines: Vec::new(),
    })
}

/// The signed-in account's name and photo, from `account/account_menu`.
pub fn account(v: &Value) -> Option<(String, Option<String>)> {
    let header = find(v, "activeAccountHeaderRenderer")?;
    let name = text(header.get("accountName"));
    (!name.is_empty()).then(|| (name, thumbnail(header.get("accountPhoto"))))
}

/// The channels in a `getAccountSwitcherEndpoint` answer, in its order,
/// with `isSelected` as `current`. A brand account carries a
/// `pageIdToken`; the Google account's own channel has none.
pub fn channels(v: &Value) -> Vec<crate::model::Channel> {
    let mut found = Vec::new();
    collect_channels(v, &mut found);
    found
}

fn collect_channels(v: &Value, found: &mut Vec<crate::model::Channel>) {
    match v {
        Value::Object(map) => {
            if let Some(item) = map.get("accountItem") {
                let name = text(item.get("accountName"));
                if !name.is_empty() {
                    let handle = Some(text(item.get("channelHandle"))).filter(|h| !h.is_empty());
                    let page_id = array(at(
                        item,
                        &[
                            "serviceEndpoint",
                            "selectActiveIdentityEndpoint",
                            "supportedTokens",
                        ],
                    ))
                    .iter()
                    .find_map(|t| at(t, &["pageIdToken", "pageId"])?.as_str())
                    .map(str::to_owned);
                    found.push(crate::model::Channel {
                        name,
                        handle,
                        photo: thumbnail(item.get("accountPhoto")),
                        page_id,
                        current: item.get("isSelected").and_then(Value::as_bool) == Some(true),
                    });
                }
                return;
            }
            map.values().for_each(|x| collect_channels(x, found));
        }
        Value::Array(list) => list.iter().for_each(|x| collect_channels(x, found)),
        _ => {}
    }
}

/// Whether YouTube treated the request as signed in.
pub fn logged_in(v: &Value) -> Option<bool> {
    array(at(v, &["responseContext", "serviceTrackingParams"]))
        .iter()
        .find_map(|p| {
            array(p.get("params"))
                .iter()
                .find(|q| q.get("key").and_then(Value::as_str) == Some("logged_in"))
                .and_then(|q| q.get("value").and_then(Value::as_str))
                .map(|v| v == "1")
        })
}

// ---- the signed-in account's state on pages (likes, library, subscriptions, ownership) ----

use crate::model::{LibraryToggle, LikeStatus, Subscription};

/// The requested song's rating, from the player's like button in `next`.
fn watch_like(v: &Value) -> Option<(String, LikeStatus)> {
    let button = array(at(
        v,
        &["playerOverlays", "playerOverlayRenderer", "actions"],
    ))
    .iter()
    .find_map(|a| a.get("likeButtonRenderer"))?;
    Some((
        str_at(button, &["target", "videoId"])?.to_owned(),
        like_status(button.get("likeStatus"))?,
    ))
}

/// What the account can do with the page's album, playlist or artist: the
/// library toggle (`BOOKMARK_BORDER`, `isToggled` when saved), the artist's
/// subscribe button, and the editable header of the account's own playlists.
fn account_header(v: &Value, page: &mut Page) {
    let Some(h) = page.header.as_mut() else {
        return;
    };
    let tabs = at(v, &["contents", "twoColumnBrowseResultsRenderer", "tabs"]);
    if let Some(editable) = tabs.and_then(|t| find(t, "musicEditablePlaylistDetailHeaderRenderer"))
    {
        h.editable = str_at(editable, &["playlistId"]).map(str::to_owned);
    }
    let buttons = tabs
        .and_then(|t| find(t, "musicResponsiveHeaderRenderer"))
        .map(|r| array(r.get("buttons")))
        .unwrap_or(&[]);
    h.library = buttons.iter().find_map(|b| {
        let toggle = b.get("toggleButtonRenderer")?;
        let id = str_at(
            toggle,
            &[
                "defaultServiceEndpoint",
                "likeEndpoint",
                "target",
                "playlistId",
            ],
        )?;
        Some(LibraryToggle {
            playlist_id: id.to_owned(),
            saved: toggle.get("isToggled").and_then(Value::as_bool) == Some(true),
        })
    });
    h.subscription = v
        .get("header")
        .and_then(|h| find(h, "subscribeButtonRenderer"))
        .and_then(|s| {
            Some(Subscription {
                channel_id: s.get("channelId")?.as_str()?.to_owned(),
                subscribed: s.get("subscribed").and_then(Value::as_bool) == Some(true),
            })
        });
}
