//! No account or network: Liked music has another page of songs and no
//! server Play/Shuffle endpoints, as the real surface does.

use encore_core::backend::{Command, Event};
use encore_core::model::{Page, Playback, PlaylistCounts, PlaylistLoad, Target};
use gpui_kit::TestAppContext;

use super::{Ui, fixture};
use crate::nav::View;

fn liked(cx: &mut TestAppContext) -> (Ui, Page) {
    let mut ui = Ui::start(cx);
    let target = Target::browse("VLLM");
    ui.app
        .update(&mut ui.cx, |app, cx| app.open(View::Page(target), cx));
    let mut page = fixture("playlist");
    let header = page.header.as_mut().expect("playlist header");
    header.title = "Synthetic liked songs".into();
    header.play = None;
    header.shuffle = None;
    let songs = page
        .shelves
        .iter_mut()
        .find(|s| s.title.is_empty())
        .expect("playlist songs");
    songs.continuation = Some("synthetic-more-songs".into());
    ui.answer_page(page.clone());
    ui.take_sent();
    (ui, page)
}

#[gpui_kit::test]
fn liked_play_loads_the_whole_playlist_instead_of_copying_visible_rows(cx: &mut TestAppContext) {
    let (mut ui, _) = liked(cx);
    ui.click("header-play");
    let commands = ui.take_sent();
    assert!(commands.iter().any(|c| matches!(c, Command::PlayPlaylist { target, start: None, shuffle: false } if target == &Target::browse("VLLM"))));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::PlayTracks { .. }))
    );
}

#[gpui_kit::test]
fn liked_shuffle_exists_without_a_server_endpoint_and_loads_the_whole_list(
    cx: &mut TestAppContext,
) {
    let (mut ui, _) = liked(cx);
    ui.click("header-shuffle");
    assert!(ui.take_sent().iter().any(|c| matches!(
        c,
        Command::PlayPlaylist {
            start: None,
            shuffle: true,
            ..
        }
    )));
}

#[gpui_kit::test]
fn clicking_a_liked_song_keeps_its_selection_and_loads_the_whole_list(cx: &mut TestAppContext) {
    let (mut ui, page) = liked(cx);
    let shelf = encore_core::account::entries(&page).expect("songs").clone();
    let item = shelf.items[1].clone();
    let video = item.track.as_ref().expect("a song").video_id.clone();
    ui.app
        .update(&mut ui.cx, |app, cx| app.activate_item(&item, &shelf, cx));
    let commands = ui.take_sent();
    assert!(
        commands.iter().any(
            |c| matches!(c, Command::PlayPlaylistSong { track, .. } if track.video_id == video)
        )
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::PlayTracks { .. }))
    );
}

#[gpui_kit::test]
fn refreshing_a_playlist_invalidates_its_complete_cache(cx: &mut TestAppContext) {
    let (mut ui, _) = liked(cx);
    ui.click("header-refresh");
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::RefreshPlaylists))
    );
}

#[gpui_kit::test]
fn metadata_progress_is_visible_without_replacing_pause_with_an_audio_spinner(
    cx: &mut TestAppContext,
) {
    let (mut ui, _) = liked(cx);
    ui.push(Event::Playback(Playback {
        playing: true,
        loading: false,
        playlist_load: Some(PlaylistLoad {
            key: Target::browse("VLLM").key(),
            entries: 198,
            unique_tracks: 198,
            counts: Some(PlaylistCounts {
                rows: 200,
                unavailable: 2,
                expected: Some(1282),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }));
    ui.find("playlist-loading");
    ui.find("playlist-progress:Loading playlist… 200 of 1282 entries checked · 2 unavailable");
    ui.find("play-button:pause");
    ui.click("cancel-playlist-loading");
    assert!(
        ui.take_sent()
            .iter()
            .any(|command| matches!(command, Command::CancelPlaylistLoad))
    );
}

#[gpui_kit::test]
fn completed_metadata_hides_the_loading_banner_and_shows_the_cache_summary(
    cx: &mut TestAppContext,
) {
    let (mut ui, _) = liked(cx);
    ui.push(Event::Playback(Playback {
        playlist_load: Some(PlaylistLoad {
            key: Target::browse("VLLM").key(),
            entries: 1246,
            unique_tracks: 1228,
            complete: true,
            cached: true,
            ..Default::default()
        }),
        ..Default::default()
    }));
    assert!(ui.bounds("playlist-loading").is_none());
    ui.find("playlist-summary");
}
