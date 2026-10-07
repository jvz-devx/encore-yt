# Encore: library power tools

Research only. No code, git, builds or YouTube requests were made. Facts come from three places:

- **Encore** (read-only): `crates/core/src/innertube.rs`, `crates/core/src/account.rs`, `crates/core/src/backend/account.rs`, `crates/core/src/backend/queue.rs`, `docs/integration.md` (dated facts, probed 2026-10-01 and 2026-10-07).
- **ytmusicapi** (the reference for the same private API), cited per fact:
  - PL = `ytmusicapi/mixins/playlists.py` (https://github.com/sigma67/ytmusicapi/blob/main/ytmusicapi/mixins/playlists.py)
  - LIB = `ytmusicapi/mixins/library.py` (same repo, `mixins/library.py`)
  - UT = `ytmusicapi/mixins/_utils.py` (sort params)
  - HLP = `ytmusicapi/helpers.py` (SAPISIDHASH)
  - CONT = `ytmusicapi/continuations.py`
  - CONST = `ytmusicapi/constants.py`
  - DOCS = https://ytmusicapi.readthedocs.io/en/stable/reference/playlists.html
- **Official web app**: only secondary press reports (see section 6). Anything not confirmed is marked "unverified".

## 1. What Encore already calls

| Endpoint | Action / body | Where in Encore | Batch? |
|---|---|---|---|
| `like/like`, `like/dislike`, `like/removelike` | `target.videoId` | `innertube.rs` `rate` | one per song |
| `like/like`, `like/removelike` | `target.playlistId` (save album/playlist) | `save_to_library` | one |
| `subscription/subscribe`, `subscription/unsubscribe` | `channelIds: [id]` | `subscribe` | one |
| `playlist/create` | `title`, `description`, `privacyStatus: PRIVATE`, optional `videoIds` | `create_playlist` | **yes**, all ids in one call |
| `browse/edit_playlist` | `ACTION_ADD_VIDEO` (`addedVideoId`), several per call | `Edit::Add`, `backend/account.rs` L136–160 | **yes**, one request for N songs |
| `browse/edit_playlist` | `ACTION_REMOVE_VIDEO` (`setVideoId`, `removedVideoId`) | `Edit::Remove` (single `set_video_id`) | one entry per call |
| `browse/edit_playlist` | `ACTION_MOVE_VIDEO_BEFORE` (`setVideoId`, `movedSetVideoIdSuccessor`) | `Edit::Move` (`before: Option`) | one per call |
| `browse/edit_playlist` | `ACTION_SET_PLAYLIST_NAME`, `ACTION_SET_PLAYLIST_DESCRIPTION` | `Edit::Details` | one |
| `playlist/delete` | `playlistId` | `delete_playlist` | one |
| `feedback` | `feedbackTokens` | `feedback` (menu tokens, e.g. library add/remove) | one |
| `next` | `videoId` / `playlistId` | watch-next, `like_status` | n/a |

Encore facts that matter here:

- `ACTION_ADD_VIDEO` without `dedupeOption` refuses a song already in the playlist (`STATUS_FAILED`, "This track is already in the playlist"). `DEDUPE_OPTION_SKIP` adds a second copy. Encore sends no option (`docs/integration.md`, "Duplicates").
- Playlist rows carry `playlistItemData.playlistSetVideoId`, the id that remove and move need (`docs/integration.md`, "Account state on pages"). A remove or move therefore needs a playlist page read that is current.
- `Edit::Add` already returns `Done::Added` with `(videoId, setVideoId)` per new entry from `playlistEditResults`. That is what a move-between-playlists chain needs.
- The backend performs account writes one at a time, in order (`docs/integration.md`, "Account changes"). Bulk work will queue behind one another.
- `AccountAction::Move` (`crates/core/src/account.rs` L158, handled L1106) and `PageEdit::Move` exist in core. The only constructors I found in `crates/app` are for `AccountAction::Add` and `AccountAction::Remove`, so drag-to-reorder of a playlist's rows appears not to be wired to a view yet. Verify before relying on it.
- The queue already has drag-to-reorder (`crates/app/src/views/queue/row.rs` L125–137, `on_drag`/`drag_over`/`on_drop` with a `DraggedSong` payload). That is the pattern to copy.
- Encore has no library sort or filter UI that I could find in `crates/app`. The only `sort` hits are unrelated code.
- Shuffle is in `crates/core/src/backend/queue.rs` (`shuffle` L205, `unshuffle` L226). It keeps "additions in order after the current song".

## 2. Playlist editing in ytmusicapi (the reference)

All playlist edits go to `browse/edit_playlist` with a list `actions`. ytmusicapi builds one action per item and sends them in one request, with no chunking or sleep anywhere in these methods (PL, `add_playlist_items`, `remove_playlist_items`, `edit_playlist`).

Action names found in PL (`edit_playlist` and helpers):

- `ACTION_ADD_VIDEO` with `addedVideoId`. Used per video by `add_playlist_items`.
- `ACTION_ADD_PLAYLIST` with `addedFullListId`, used by `add_playlist_items(source_playlist=…)`. If `videoIds` are empty, ytmusicapi appends an `ACTION_ADD_VIDEO` with `addedVideoId: None` so YouTube Music returns the `setVideoId` mapping (PL).
- `ACTION_REMOVE_VIDEO` with `setVideoId` and `removedVideoId`. `remove_playlist_items` filters entries that lack `videoId` or `setVideoId`. It does not look up `setVideoId` itself; the caller passes `PlaylistItem`s from `get_playlist()` (PL).
- `ACTION_MOVE_VIDEO_BEFORE`: `edit_playlist(moveItem=…)`. A string is one `setVideoId`. A tuple `(setVideoId, successorSetVideoId)` also sets `movedSetVideoIdSuccessor` (PL).
- `ACTION_SET_PLAYLIST_NAME`, `ACTION_SET_PLAYLIST_DESCRIPTION`, `ACTION_SET_PLAYLIST_PRIVACY`, `ACTION_SET_PLAYLIST_VIDEO_ORDER`, `ACTION_SET_ADD_TO_TOP`, `ACTION_SET_ALLOW_ITEM_VOTE`, `ACTION_CREATE_COLLABORATION_INVITE_LINK`, `ACTION_SET_CLOSED_TO_CONTRIBUTIONS` (PL, `edit_playlist`).
- `edit_playlist(addPlaylistId=…)` sends `ACTION_ADD_PLAYLIST` (PL). `create_playlist(source_playlist=…)` exists in the signature (PL), but I did not confirm its body from the source.
- `edit_playlist(sortOrder=…)`: "Change the order tracks are returned in. The default is MANUAL." (DOCS). Whether this changes the stored order or only the view is not stated in the sources. Treat it as unverified.
- Endpoints in PL: `browse` with `VL` + playlistId (reads), `browse/edit_playlist`, `playlist/create`, `playlist/delete`.

Duplicates: `add_playlist_items(duplicates=False)` omits `dedupeOption`. `duplicates=True` sets `DEDUPE_OPTION_SKIP` on every add action (PL). This matches Encore's documented finding.

Reads: `get_playlist(limit=100)`. "None retrieves them all." Continuations loop while a token exists and `limit is None or len(items) < limit` (CONT, `get_continuations_2025`). The page size is not set in the code I read, so the request count for a long playlist is unknown. Only a probe would tell it.

Sorting in ytmusicapi's library (LIB and UT):

- `get_library_songs` (browseId `FEmusic_liked_videos`), `get_library_albums` (`FEmusic_liked_albums`), `get_library_artists` (`FEmusic_library_corpus_track_artists`). Default `limit` 25. All use `browse`.
- The `order` argument is one of `a_to_z`, `z_to_a`, `recently_added`. It is sent only as `params` on the `browse` body, with values `ggMGKgQIARAA`, `ggMGKgQIARAB`, `ggMGKgQIABAB` (UT, `prepare_order_params`). No separate sort endpoint or `orderBy` field is sent (LIB, UT).
- The code does not say the sort happens on YouTube's server, but the value is in the request, so the sort is server-side. Unverified beyond that inference.

Like, subscribe and history (for context): `rate_song` uses `like/like` etc. with `target.videoId` (LIB); `subscribe_artists` is deprecated in favour of `subscribe_artist`; `unsubscribe_artists` uses `subscription/unsubscribe` with `channelIds` (LIB); `remove_history_items` uses `feedback` with `feedbackTokens` (LIB); `add_history_item` pings `videostatsPlaybackUrl` with `ver=2&c=WEB_REMIX&cpn` (LIB). None of these are a blocker.

Auth: `get_authorization` builds `SAPISIDHASH <ts>_<sha1("<ts> <SAPISID> <origin>")>`, origin `https://music.youtube.com` (HLP; CONST `YTM_DOMAIN`). Encore already does the same.

Rate limits: neither ytmusicapi's playlist code nor its docs mention a limit, throttle or retry (PL, DOCS). A web search turned up only generic 429 advice from other services. So **the per-call and per-minute limits are unknown**. Encore's own note says the account has been rate limited before (project memory), so treat limits as a real constraint.

## 3. Feature analysis

Effort: small = a few hundred lines, existing core paths; medium = new UI state or a new multi-step core flow; large = new persistence or a new surface.

### 3.1 Multi-select of songs (shift-click, ctrl-click)

- **API:** none. Pure client state.
- **Web app:** unverified. I found no source that confirms multi-select on music.youtube.com.
- **Encore:** no selection state found in `crates/app`. Song rows exist in the queue (`views/queue/row.rs`) and in pages.
- **Effort:** medium. Needs a selection model per list (anchor, range with shift, toggle with ctrl), keyboard behaviour that matches the existing key bindings, and a "selected N" bar. Covered by the existing headless UI test harness (`crates/app/src/ui_tests/`).

### 3.2 Drag to reorder within an owned playlist

- **API:** `browse/edit_playlist` with `ACTION_MOVE_VIDEO_BEFORE`, `setVideoId` and `movedSetVideoIdSuccessor` (Encore: `Edit::Move`; ytmusicapi: `moveItem`). One request per moved entry. Moving "to the end" omits the successor (`docs/integration.md`).
- **Batching:** ytmusicapi sends one move per call. Multiple `ACTION_MOVE_VIDEO_BEFORE` items in one `actions` array is plausible from the protocol shape but **not confirmed**. Test with a self-undoing pair first.
- **Web app:** playlist "Manual" order exists; drag mechanics on web were not confirmed by any source I found. Playlist track sorting by Title, Artist, Album, Top Voted, Newest or Oldest is reported on Android only (Android Authority, see section 6).
- **Encore:** core path exists (`AccountAction::Move`, `PageEdit::Move` with an optimistic reorder in `account.rs` L1240–1310). UI drag is the missing part. Queue row drag (`views/queue/row.rs`) is the template.
- **Effort:** medium. Core is done; UI drag with a drop line and optimistic reorder. Reorder checks against a fresh read, since `setVideoId`s are per entry.
- **Request count:** one per moved song. Moving a block of 10 songs = up to 10 requests, or 10 sequential moves in the account queue. Each move's predecessor is the one below it; you have to send them in the right order.

### 3.3 Drag songs onto a playlist in the sidebar

- **API:** `browse/edit_playlist` with `ACTION_ADD_VIDEO`, one action per song, all in one request (already done by `Edit::Add`).
- **Encore:** `AccountAction::Add { playlist_id, tracks }` exists and is built from menus (`crates/app/src/account.rs` L325, `desktop/submenu.rs` L157). The sidebar drop target and the drag payload are new. Sidebar playlists are listed in `crates/app/src/sidebar.rs`.
- **Duplicate handling:** the refusal comes back as `STATUS_FAILED` and the existing duplicate message in `account.rs` L1191 already reports it. Partial adds of a batch are not described in the sources, so check what YouTube Music does when some ids in one batch are duplicates (unverified).
- **Effort:** medium (drop target and drag payload; the core call exists).

### 3.4 Bulk add and bulk remove

- **Bulk add:** one request with N `ACTION_ADD_VIDEO` (`Edit::Add` already takes `Vec`). The batch size limit is **unknown**. ytmusicapi does not chunk (PL). Chunk at a conservative size of your own choosing, e.g. 50, and say so in code; do not claim YouTube's limit.
- **Bulk remove:** the core takes a single `set_video_id` (`Edit::Remove`). ytmusicapi sends a list of `ACTION_REMOVE_VIDEO` items in one request (PL), so this should be one request for N songs. Same unknown batch size, same chunking.
- **Effort:** bulk add small (once multi-select exists); bulk remove small (change `Remove` from one id to a list; the menu uses one id today, `desktop/menu.rs` L244).
- **Requests:** add N songs = 1 request (or ceil(N/chunk)); remove N = 1 request (or ceil(N/chunk)). Each needs `setVideoId`s from a current read.

### 3.5 Move songs from one playlist to another

- **API:** add to the target (`ACTION_ADD_VIDEO`, returns `setVideoId`s in `playlistEditResults`), then remove from the source (`ACTION_REMOVE_VIDEO` with the `setVideoId` of the **source** entry). Encore already reads the added ids (`backend/account.rs` L136–160).
- **Failure modes:** add succeeds and remove fails = the song is in both playlists. Remove first and add fails = song lost from the source. Add first is the safe order. The sources do not describe partial failure inside one batch.
- **Effort:** medium. Two-step core flow with a clear failure message and an "undo" that is itself two requests.
- **Requests:** 2 per batch (1 add + 1 remove), or 2 × ceil(N/chunk).

### 3.6 Sorting and filtering in Library lists and inside a playlist

Server-side (library, not playlists):

- YouTube's own library sort, per ytmusicapi UT: "Recently added" (default), "A to Z", "Z to A" for liked songs, liked albums and artists. Sent as `params` on `browse`. Each sort is one new browse call, with continuations for more pages (LIB, CONT).
- Web app: the 2019–2020 reports say playlists, saved albums, liked songs and subscribed artists can be sorted this way on mobile and desktop web (9to5Google, Android Police, Android Central; see section 6). Date of those reports: 2019 to 2020. Unverified for today's app.
- Effort: medium. Needs a sort menu, a re-fetch per sort, and handling of continuations so that sorting a long list does not return a partial result.

Client-side:

- Sort and filter over the rows already loaded (title, artist, album, duration, added order where present). Quick and free in requests, but **partial for long lists**: until all continuations are loaded, the sort is wrong. Either load all pages first (a request per page, see section 4) or label the view "loaded so far".
- Filter by text: client-side on loaded rows; cheap.

Inside a playlist:

- ytmusicapi exposes `edit_playlist(sortOrder=…)` for playlist-level order (default `MANUAL`) (DOCS). This changes how the playlist is returned or stored. Unverified; do not expose it without a test on a throwaway playlist.
- Client-side sorting of the view (not the stored order) costs no requests.
- Effort: small (client-side sort and filter on loaded rows); medium (server-side library sort with continuations).

### 3.7 Duplicates within a playlist or across playlists

- **Detect:** within a playlist, group rows by `videoId` from one full read (`get_playlist` with all continuations). Rows carry `videoId` and `playlistSetVideoId`, so a removal needs no extra lookup.
- **Remove:** `ACTION_REMOVE_VIDEO` per duplicate's `setVideoId`, batched in one request (section 3.4). Keep the first occurrence by default, let the user choose.
- **Across playlists:** one full read per playlist, then group by `videoId` across them. A 20-playlist library = 20+ reads (plus continuations). The reads lag writes by up to ~25 s (`docs/integration.md`), so a dedupe run right after edits can show stale duplicates.
- **Version ambiguity:** the same song can be a different `videoId` (a live or a remix). Matching by title/artist/duration would be a fuzzy guess; say so in the UI or match on `videoId` only.
- **Effort:** small for within one playlist (uses reads and bulk remove); medium for across playlists (read cost, progress UI, a cache with a time limit).

### 3.8 Smart shuffle (avoid the same artist twice in a row, or weight recent plays down)

- **API:** none. Purely client-side on the queue.
- **Encore:** `crates/core/src/backend/queue.rs` `shuffle` (L205) takes `current` and builds play order. It keeps "additions in order after the current song". A smarter shuffle reorders the same list in memory. Needs an artist (and, for recent-play weighting, a play history) per entry. Play history: `report_play` already sends history pings (`innertube.rs`); a local record of recent plays would be new state.
- **Requests:** 0.
- **Effort:** medium. The shuffle function itself is small; the artist key (channel id vs name), tie handling, and tests are the work. Artist-spread is simpler and needs no history.
- **Risk:** the queue and shuffle state are saved in `session.json` (`docs/integration.md`); a changed shuffle must keep session restore working.

### 3.9 Other things the API makes easy

| Feature | Mechanism | Requests | Effort | Notes |
|---|---|---|---|---|
| Add whole album to a playlist | `browse/edit_playlist` `ACTION_ADD_PLAYLIST` with `addedFullListId` = the album's `OLAK5uy_…` id, or `add_playlist_items(source_playlist=…)` (PL) | 1 | small | "no duplicate check" per PL docstring. Album id is already in the album page (`docs/integration.md`). Confirm the action works on an album id, not only a playlist id (unverified). |
| Merge playlist A into B | same as above, source = A's `VL` id | 1 | small | Same caveats. Duplicates are not removed; dedupe after with section 3.7. |
| Copy playlist | `create_playlist(source_playlist=…)` (signature in PL) or create + `ACTION_ADD_PLAYLIST` | 1–2 | small | Create body not confirmed from the source. Probe on a throwaway playlist. |
| Clear playlist | `ACTION_REMOVE_VIDEO` for all rows, one request (or chunks) | 1 per chunk | small | Cheaper than delete + recreate, which changes the playlist id and breaks links. |
| Set privacy | `ACTION_SET_PLAYLIST_PRIVACY` (PL) | 1 | small | Values not in the sources; check the exact enum before use. |
| Rename / describe | `ACTION_SET_PLAYLIST_NAME`, `ACTION_SET_PLAYLIST_DESCRIPTION` | 1 | done | Already in `Edit::Details`. |
| Save/unsave album or playlist | `like/like`, `like/removelike` with `target.playlistId` | 1 | done | Already in Encore. |
| Subscribe/unsubscribe artist | `subscription/*` | 1 | done | Already in Encore. |
| Undo last bulk edit | Inverse action (add ↔ remove; move back to the old predecessor) | same as the edit | small–medium | Needs the old `setVideoId`s and predecessor. Encore's optimistic layer already keeps "was" values (`account.rs` L1024). |

## 4. Request counts and rate-limit risk

What is known:

- ytmusicapi sends one request per batch and does no chunking or sleeping in the playlist methods (PL). YouTube's actual limits are not documented in any source I found.
- Encore runs account writes one at a time (`docs/integration.md`). A 100-song bulk add in one batch is 1 request; 100 moves are 100 requests, each waiting on the last.
- Reads lag writes by up to ~25 s (`docs/integration.md`). A verification read right after a bulk write may not show the result. Verify with a delay or re-read once.
- The account has been rate limited before (project memory). Keep reads few.

Counts per feature (for planning; batch size `B` is a chosen chunk, not a YouTube limit):

| Operation | Requests |
|---|---|
| Add N songs | ceil(N/B) |
| Remove N songs | ceil(N/B) |
| Move N songs (reorder) | N (one per moved entry) |
| Move N songs to another playlist | 2·ceil(N/B) |
| Read a playlist with P items | ceil(P/page size) + 1; page size unknown |
| Dedupe one playlist | the read above + ceil(D/B) removes |
| Dedupe across K playlists | sum of reads + removes; ≥ K requests |
| Library sort, one list of P items | ceil(P/page size) per sort; each sort change repeats it |

Suggested rules for the build:

- Chunk every bulk write at one size, chosen and written down in code, and start small (e.g. 25–50). Measure the reaction on the test account only, without real streams.
- Probe `ACTION_MOVE_VIDEO_BEFORE` with several actions per request on a throwaway playlist before shipping bulk reorder.
- Cache playlist reads for the session and invalidate on the app's own writes.
- Never retry a refused write blindly. A `STATUS_FAILED` (duplicate, unknown id) is an answer; an HTTP 429 or 5xx needs backoff, and the backoff rule is not in any source yet, so design it.
- Use the account's test rules from `AGENTS.md`: a fresh `XDG_CONFIG_HOME`/`XDG_CACHE_HOME` for probes, and no real-account writes beyond self-undoing pairs.

## 5. Risks

- **Unknown limits:** batch size, requests per minute, and per-playlist item cap. The sources do not state them. Each needs a probe, and probes cost requests on a rate-limited account.
- **Stale `setVideoId`s:** remove and move need the current read. A remove on a stale id will fail or hit the wrong entry (unverified which). Re-read before any bulk remove or move.
- **Partial batches:** no source says what happens when one action in a batch fails. Treat the batch answer as per-action where the response allows it; otherwise re-read.
- **Read lag:** up to ~25 s after writes (`docs/integration.md`). Dedupe and move checks must allow for it.
- **Private API drift:** YouTube changes these endpoints. ytmusicapi is an external reference, not a contract. Encore's `docs/integration.md` and the parser fixtures (`crates/core/tests/parse_fixtures.rs`) are the place to record what was verified.
- **Duplicate policy:** `DEDUPE_OPTION_SKIP` would add a second copy. Keep it out of every path unless a feature wants duplicates.
- **Playlist sort side effects:** `sortOrder` may change stored order. Unverified; do not expose before a test.

## 6. Official web app: what the sources say

- Library sorting (Recently added, A to Z, Z to A) on web, for playlists, saved albums, liked songs and subscribed artists: Android Police, 2020-10-27 (https://www.androidpolice.com/2020/10/27/youtube-music-adds-new-library-sorting-options-on-the-web/) and 9to5Google, 2020-10-28 (https://9to5google.com/2020/10/28/youtube-music-gains-the-ability-to-sort-your-library-alphabetically/). Secondary press, dated.
- Playlist track sorting (Manual, Top Voted, Newest, Oldest, and new Title, Artist, Album) rolling out, first seen on Android 9.20.52 (Android Authority, https://www.androidauthority.com/youtube-music-playlist-sorting-options-3670873/). Gradual rollout; web status not stated.
- Multi-select, drag-to-reorder and drag-to-playlist on the web app: **not confirmed by any source I could reach.** Check the web app on a signed-out or test account, not by a request from Encore.

## 7. Recommended first set (3 to 5), ordered by value for effort

1. **Multi-select plus bulk add to a playlist.** Core already batches adds in one request (`Edit::Add`), so the work is the selection model and a "Add N to playlist" action. Highest value, low request cost (1 per batch). Effort: medium (UI), small (core).
2. **Bulk remove and clear playlist.** Change `Edit::Remove` from one id to a list. One request for N removes. Reuses the selection from step 1. Effort: small. Needs a fresh read first.
3. **Duplicate finder within one playlist.** One read plus one bulk remove; no new endpoint. Matches the documented duplicate behaviour. Effort: small. Skip cross-playlist for now (read cost).
4. **Drag songs onto a sidebar playlist.** Uses the existing `AccountAction::Add`, adds only a drop target and payload (copy the queue row drag). Effort: medium. No new requests beyond the add.
5. **Smart shuffle (artist spread) and client-side sort/filter of loaded rows.** No requests at all, no API risk. Effort: medium (shuffle), small (sort/filter). Good way to ship value while the account-write work waits on probes.

Defer: drag-to-reorder within a playlist (needs a probe of multi-move batches and a stable read; core already exists, so it can follow soon after), move-between-playlists (failure-mode design), server-side library sort (continuations), and cross-playlist dedupe (read cost).

## 8. Open questions to settle by probing (test account, self-undoing pairs, few requests)

- Max actions per `browse/edit_playlist` request (add and remove), and what the answer looks like when one of many fails.
- Whether several `ACTION_MOVE_VIDEO_BEFORE` actions work in one request.
- Whether `ACTION_ADD_PLAYLIST` accepts an album id (`OLAK5uy_…`) as `addedFullListId`.
- The exact body of `create_playlist(source_playlist=…)` (ytmusicapi's create code was not read).
- Page size of playlist reads (`get_playlist`) and of library reads.
- What `edit_playlist(sortOrder=…)` changes: stored order or view.
- Whether the library sort params (`ggMGKgQIARAA` etc., from ytmusicapi) still work with Encore's `WEB_REMIX` client version `1.20260923.01.00`. They come from a third-party file and may be stale.
