//! The player bar follows the backend's reports: the song, play or pause,
//! and the times.

use gpui_kit::TestAppContext;
use ytfast::backend::{Command, Event};
use ytfast::model::{Playback, Track};

use super::home::{on_home, quick_picks};

#[gpui_kit::test]
fn the_player_bar_shows_what_the_backend_reports(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (_, shelf) = quick_picks(&home);
    let queue: Vec<Track> = shelf.items.iter().filter_map(|i| i.track.clone()).collect();
    let song = queue[1].clone();
    ui.find("play-button:play");

    ui.push(Event::Queue(queue));
    ui.push(Event::Playback(Playback {
        index: Some(1),
        playing: false,
        position: 65.0,
        duration: 200.0,
        volume: 70.0,
        ..Default::default()
    }));
    ui.find(&format!("bar-title:{}", song.title));
    ui.find("play-button:play");
    ui.find("bar-elapsed:1:05");
    ui.find("bar-length:3:20");

    ui.push(Event::Playback(Playback {
        index: Some(1),
        playing: true,
        position: 66.0,
        duration: 200.0,
        volume: 70.0,
        ..Default::default()
    }));
    ui.find("play-button:pause");
    assert!(ui.bounds("play-button:play").is_none());

    ui.push(Event::Playback(Playback {
        index: Some(1),
        loading: true,
        duration: 200.0,
        volume: 70.0,
        ..Default::default()
    }));
    ui.find("play-button:loading");

    ui.take_sent();
    ui.click("play-button:loading");
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::TogglePause)),
        "the play button sends TogglePause"
    );
}
