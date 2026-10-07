//! A right-click on a song opens its menu, and Play next queues it.

use gpui_kit::TestAppContext;
use ytfast::backend::Command;

use super::home::{home_row, on_home, quick_picks};

#[gpui_kit::test]
fn right_click_opens_the_song_menu_and_play_next_queues_it(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (q, shelf) = quick_picks(&home);
    let song = shelf.items[2].track.clone().expect("a song row");
    assert!(ui.bounds("menu-entry:Play next").is_none());

    ui.right_click(&home_row(q, 2));

    let open = ui
        .app
        .read_with(&ui.cx, |app, _| app.desktop.layers.menu.is_some());
    assert!(open, "the menu is open");
    ui.find("menu-entry:Play next");
    ui.find("menu-entry:Add to queue");
    ui.click("menu-entry:Play next");

    let sent = ui.take_sent();
    let queued = sent.iter().find_map(|c| match c {
        Command::PlayNext(tracks) => Some(tracks),
        _ => None,
    });
    let queued = queued.expect("Play next sends PlayNext");
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].video_id, song.video_id);
    let open = ui
        .app
        .read_with(&ui.cx, |app, _| app.desktop.layers.menu.is_some());
    assert!(!open, "choosing closes the menu");
    assert!(ui.bounds("menu-entry:Play next").is_none());
}
