//! The real player bar and picker on the fake backend, with loopback-device
//! summaries. UI input never starts network discovery or a real receiver.

use encore_core::backend::{Command, Event};
use encore_core::casting::{Device, Kind, Session, State};
use encore_core::model::Playback;
use gpui_kit::{Modifiers, MouseButton, TestAppContext};

use super::Ui;
use super::home::{on_home, quick_picks};

fn devices() -> Vec<Device> {
    vec![
        Device {
            id: "fake-cast".into(),
            kind: Kind::Cast,
            name: "Local Cast receiver".into(),
            model: "Test".into(),
            group: false,
        },
        Device {
            id: "fake-dlna".into(),
            kind: Kind::Dlna,
            name: "Local DLNA renderer".into(),
            model: "Test".into(),
            group: false,
        },
    ]
}

fn playing(cx: &mut TestAppContext) -> Ui {
    let (mut ui, home) = on_home(cx);
    let (_, shelf) = quick_picks(&home);
    ui.push(Event::Queue(
        shelf.items.iter().filter_map(|i| i.track.clone()).collect(),
    ));
    ui.push(Event::Playback(Playback {
        index: Some(0),
        playing: true,
        position: 25.0,
        duration: 180.0,
        volume: 50.0,
        ..Default::default()
    }));
    ui.take_sent();
    ui
}

#[gpui_kit::test]
fn cast_picker_scans_lists_both_protocols_and_closes_with_escape(cx: &mut TestAppContext) {
    let mut ui = playing(cx);
    ui.click("cast-button");
    ui.find("cast-picker");
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::CastScan(true)))
    );
    ui.push(Event::Cast(State {
        scanning: true,
        ..Default::default()
    }));
    ui.find("cast-scanning");
    ui.push(Event::Cast(State {
        devices: devices(),
        ..Default::default()
    }));
    ui.find("cast-device:Local Cast receiver");
    ui.find("cast-device:Local DLNA renderer");
    ui.keys("escape");
    assert!(ui.bounds("cast-picker").is_none());
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::CastScan(false)))
    );
}

#[gpui_kit::test]
fn choosing_a_device_shows_connecting_then_the_remote_and_stop(cx: &mut TestAppContext) {
    let mut ui = playing(cx);
    ui.push(Event::Cast(State {
        devices: devices(),
        ..Default::default()
    }));
    ui.click("cast-button");
    ui.take_sent();
    ui.click("cast-device:Local Cast receiver");
    assert!(ui.take_sent().iter().any(|c| matches!(c, Command::CastConnect { id, kind: Kind::Cast, takeover: false } if id == "fake-cast")));
    ui.find("cast-status:Connecting…");
    ui.push(Event::Cast(State {
        devices: devices(),
        scanning: true,
        ..Default::default()
    }));
    ui.find("cast-status:Connecting…");
    ui.push(Event::Cast(State {
        devices: devices(),
        session: Some(Session::Connecting(devices().remove(0))),
        ..Default::default()
    }));
    ui.find("cast-status:Connecting…");
    ui.push(Event::Cast(State {
        devices: devices(),
        session: Some(Session::Active(devices().remove(0))),
        ..Default::default()
    }));
    ui.find("cast-status:Playing on Local Cast receiver");
    let stop = ui.find("cast-stop");
    ui.push(Event::Cast(State {
        devices: devices(),
        scanning: true,
        session: Some(Session::Active(devices().remove(0))),
        ..Default::default()
    }));
    assert_eq!(
        ui.find("cast-stop"),
        stop,
        "refreshing must not move the stop button under the pointer"
    );
    ui.click("cast-stop");
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::CastDisconnect))
    );
    ui.push(Event::Cast(State {
        devices: devices(),
        ..Default::default()
    }));
    assert!(
        ui.bounds("cast-status:Playing on Local Cast receiver")
            .is_none()
    );
}

#[gpui_kit::test]
fn casting_keeps_transport_seek_and_volume_on_the_existing_controls(cx: &mut TestAppContext) {
    let mut ui = playing(cx);
    ui.push(Event::Cast(State {
        session: Some(Session::Active(devices().remove(1))),
        ..Default::default()
    }));
    ui.click("play-button:pause");
    ui.keys("shift-right");
    ui.keys("shift-left");
    ui.keys("right");
    ui.keys("+");
    let commands = ui.take_sent();
    assert!(commands.iter().any(|c| matches!(c, Command::TogglePause)));
    assert!(commands.iter().any(|c| matches!(c, Command::Next)));
    assert!(commands.iter().any(|c| matches!(c, Command::Previous)));
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::Seek(s) if (s - 30.0).abs() < 1.0))
    );
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::Volume(v) if *v == 55.0))
    );
    // Acknowledge the seek first. The UI intentionally holds the requested
    // position over older reports until it sees the matching remote position.
    ui.push(Event::Playback(Playback {
        index: Some(0),
        playing: false,
        position: 30.0,
        duration: 180.0,
        ..Default::default()
    }));
    ui.push(Event::Playback(Playback {
        index: Some(0),
        playing: false,
        position: 35.0,
        duration: 180.0,
        ..Default::default()
    }));
    ui.find("play-button:play");
    ui.find("bar-elapsed:0:35");
}

#[gpui_kit::test]
fn a_discovery_refresh_during_a_row_click_still_connects_the_same_device(cx: &mut TestAppContext) {
    let mut ui = playing(cx);
    ui.push(Event::Cast(State {
        devices: devices(),
        ..Default::default()
    }));
    ui.click("cast-button");
    ui.take_sent();
    let at = ui.find("cast-device:Local Cast receiver").center();
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    ui.cx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    ui.frame();
    ui.push(Event::Cast(State {
        devices: devices(),
        scanning: true,
        ..Default::default()
    }));
    ui.cx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    ui.frame();
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::CastConnect { id, .. } if id == "fake-cast"))
    );
}

#[gpui_kit::test]
fn a_device_appearing_during_a_click_cannot_redirect_it_to_another_receiver(
    cx: &mut TestAppContext,
) {
    let mut ui = playing(cx);
    ui.push(Event::Cast(State {
        devices: vec![devices().remove(0)],
        ..Default::default()
    }));
    ui.click("cast-button");
    ui.take_sent();
    let at = ui.find("cast-device:Local Cast receiver").center();
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    ui.cx
        .simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    ui.frame();
    let mut refreshed = devices();
    refreshed.reverse();
    ui.push(Event::Cast(State {
        devices: refreshed,
        ..Default::default()
    }));
    ui.cx
        .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    ui.frame();
    let sent = ui.take_sent();
    assert!(
        !sent
            .iter()
            .any(|c| matches!(c, Command::CastConnect { id, .. } if id != "fake-cast")),
        "a refresh cannot substitute another device under a held click"
    );
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::CastConnect { id, .. } if id == "fake-cast")),
        "the visible receiver stayed under the pointer, so the held click must not vanish"
    );
}

#[gpui_kit::test]
fn busy_receiver_needs_an_explicit_replace_and_empty_player_cannot_connect(
    cx: &mut TestAppContext,
) {
    let mut ui = playing(cx);
    ui.push(Event::Cast(State {
        devices: devices(),
        session: Some(Session::Confirm {
            device: devices().remove(0),
            app: "Another app".into(),
        }),
        ..Default::default()
    }));
    ui.find("cast-confirm");
    ui.take_sent();
    ui.click("cast-replace");
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::CastConnect { takeover: true, .. }))
    );
    ui.push(Event::Cast(State {
        devices: devices(),
        ..Default::default()
    }));
    ui.push(Event::Playback(Playback::default()));
    ui.find("cast-needs-song");
    ui.take_sent();
    ui.click("cast-device:Local DLNA renderer");
    assert!(
        !ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::CastConnect { .. }))
    );
}
