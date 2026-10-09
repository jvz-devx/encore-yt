//! The player bar follows the backend's reports: the song, play or pause,
//! and the times.

use encore_core::backend::{Command, Event};
use encore_core::model::{Playback, Track};
use gpui_kit::TestAppContext;

use super::home::{on_home, quick_picks};

#[gpui_kit::test]
fn visualizer_controls_follow_the_pointer(cx: &mut TestAppContext) {
    let (mut ui, _) = on_home(cx);
    ui.push(Event::Playback(Playback {
        volume: 70.0,
        ..Default::default()
    }));
    let outside = gpui_kit::point(gpui_kit::px(-10.), gpui_kit::px(-10.));
    ui.cx
        .simulate_mouse_move(outside, None, gpui_kit::Modifiers::none());
    ui.keys("v");
    assert!(ui.bounds("visualizer-menu").is_none());
    assert!(ui.bounds("visualizer-controls").is_none());

    let inside = ui.find("visualizer").center();
    ui.cx
        .simulate_mouse_move(inside, None, gpui_kit::Modifiers::none());
    ui.frame();
    ui.find("visualizer-menu");
    ui.find("visualizer-volume:70");
    ui.take_sent();
    ui.click("visualizer-next");
    assert!(ui.take_sent().iter().any(|c| matches!(c, Command::Next)));
    ui.click("volume-slider");
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::Volume(_)))
    );

    ui.cx
        .simulate_mouse_move(outside, None, gpui_kit::Modifiers::none());
    ui.frame();
    assert!(ui.bounds("visualizer-menu").is_none());
    assert!(ui.bounds("visualizer-controls").is_none());

    ui.cx
        .simulate_mouse_move(inside, None, gpui_kit::Modifiers::none());
    ui.frame();
    ui.find("visualizer-controls");
    ui.keys("escape");
    ui.cx
        .simulate_mouse_move(outside, None, gpui_kit::Modifiers::none());
    ui.keys("v");
    assert!(ui.bounds("visualizer-controls").is_none());
}

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
