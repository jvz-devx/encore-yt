//! Home from the saved Home response: its shelves are drawn, and a click on
//! a song row plays that row's shelf from that song.

use gpui_kit::{TestAppContext, px};
use ytfast::backend::Command;
use ytfast::model::{Page, Shelf, Target};

use super::{Ui, WINDOW, fixture};

fn home_key() -> String {
    Target::browse("FEmusic_home").key()
}

pub fn quick_picks(home: &Page) -> (usize, &Shelf) {
    home.shelves
        .iter()
        .enumerate()
        .find(|(_, s)| s.title == "Quick picks")
        .expect("the Home fixture has Quick picks")
}

/// The name of song row `i` of shelf `shelf` on Home (`views::page::row`).
pub fn home_row(shelf: usize, i: usize) -> String {
    format!("{}:row:{shelf}:{i}", home_key())
}

/// The app with Home answered from the fixture, and nothing sent yet.
pub fn on_home(cx: &mut TestAppContext) -> (Ui, Page) {
    let mut ui = Ui::start(cx);
    let home = fixture("home");
    ui.answer_page(home.clone());
    ui.take_sent();
    (ui, home)
}

#[gpui_kit::test]
fn home_draws_its_shelves_from_the_fixture(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    let asked = ui.take_sent();
    assert!(
        asked
            .iter()
            .any(|c| matches!(c, Command::Page { target, .. } if target.key() == home_key())),
        "the app asks for Home as it opens"
    );
    let home = fixture("home");
    ui.answer_page(home.clone());

    // The shelves are drawn in order, top to bottom, as far as the window
    // reaches; the virtual list leaves the rest out.
    let mut drawn = Vec::new();
    for shelf in &home.shelves {
        match ui.bounds(&format!("shelf:{}", shelf.title)) {
            Some(bounds) => drawn.push((shelf.title.as_str(), bounds)),
            None => break,
        }
    }
    assert!(
        drawn.len() >= 2,
        "drawn shelves: {:?}",
        drawn.iter().map(|(t, _)| t).collect::<Vec<_>>()
    );
    for pair in drawn.windows(2) {
        assert!(
            pair[0].1.origin.y < pair[1].1.origin.y,
            "{} is drawn above {}",
            pair[0].0,
            pair[1].0
        );
    }
    let (_, first) = drawn[0];
    assert!(
        first.origin.y < px(WINDOW.1 / 2.),
        "first shelf at {first:?}"
    );

    // Quick picks shows its songs as rows, with room for a title.
    let (q, _) = quick_picks(&home);
    let title = ui.find("shelf:Quick picks");
    let row = ui.find(&home_row(q, 0));
    assert!(row.size.height >= px(40.), "row {row:?}");
    assert!(row.origin.y > title.origin.y, "rows sit under the title");
}

#[gpui_kit::test]
fn clicking_a_song_row_plays_its_shelf_from_that_song(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (q, shelf) = quick_picks(&home);
    let expected: Vec<String> = shelf
        .items
        .iter()
        .filter_map(|i| i.track.as_ref().map(|t| t.video_id.clone()))
        .collect();

    ui.click(&home_row(q, 1));

    let sent = ui.take_sent();
    let played = sent.iter().find_map(|c| match c {
        Command::PlayTracks { tracks, start } => Some((tracks, *start)),
        _ => None,
    });
    let (tracks, start) = played.expect("a click on a song row sends PlayTracks");
    let ids: Vec<&str> = tracks.iter().map(|t| t.video_id.as_str()).collect();
    assert_eq!(ids, expected, "the queue is the shelf's songs");
    assert_eq!(start, 1, "it starts at the clicked song");
}
