//! Hover intent (M28): resting on a card loads its page, passing over it
//! or scrolling doesn't, a click joins the fetch, and the budget holds.

use std::time::Duration;

use encore_core::backend::{Command, Event};
use encore_core::model::{Item, Page, Target};
use gpui_kit::{Modifiers, TestAppContext, point, px};

use super::home::on_home;
use super::{Ui, WINDOW, fixture};
use crate::pages::{DWELL, FETCHES_PER_MINUTE, RESOLVES_PER_MINUTE, Want};

fn home_key() -> String {
    Target::browse("FEmusic_home").key()
}

#[gpui_kit::test]
fn page_cache_is_bounded_without_evicting_navigation_history(cx: &mut TestAppContext) {
    let (mut ui, _) = on_home(cx);
    let app = ui.app.clone();
    ui.cx.update(|_, cx| {
        app.update(cx, |app, _| {
            let remembered = Target::browse("remembered");
            app.ensure_page(remembered.clone(), false);
            app.pages
                .history
                .push(crate::nav::View::Page(remembered.clone()));
            for n in 0..300 {
                app.ensure_page(Target::browse(format!("synthetic-{n}")), false);
            }
            assert_eq!(app.pages.states.len(), 128);
            assert!(app.pages.states.contains_key(&remembered.key()));
            assert!(app.pages.states.contains_key(&home_key()));
            assert!(
                app.pages
                    .states
                    .contains_key(&Target::browse("synthetic-299").key())
            );
        })
    });
}

/// The page keys asked for in `sent`.
fn fetched(sent: &[Command]) -> Vec<String> {
    sent.iter()
        .filter_map(|c| match c {
            Command::Page { target, .. } => Some(target.key()),
            _ => None,
        })
        .collect()
}

/// The songs prepared in `sent`.
fn prepared(sent: &[Command]) -> Vec<String> {
    sent.iter()
        .filter_map(|c| match c {
            Command::Prepare(id) => Some(id.clone()),
            _ => None,
        })
        .collect()
}

/// The first card drawn on Home that opens an album or playlist: its name
/// and its item.
fn page_card(ui: &mut Ui, home: &Page) -> (String, Item) {
    for (s, shelf) in home.shelves.iter().enumerate() {
        for (i, item) in shelf.items.iter().enumerate() {
            let name = format!("{}:card:{s}:{i}", home_key());
            if matches!(Want::play(item), Some(Want::FirstSong(_))) && ui.bounds(&name).is_some() {
                return (name, item.clone());
            }
        }
    }
    panic!("Home draws no album or playlist card");
}

fn page_key(item: &Item) -> String {
    item.target.as_ref().expect("a card's target").key()
}

fn point_at(ui: &mut Ui, name: &str) {
    let at = ui.find(name).center();
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    ui.frame();
}

/// The pointer over the top bar's empty middle, away from any card.
fn point_away(ui: &mut Ui) {
    let at = point(px(WINDOW.0 / 2.), px(8.));
    ui.cx.simulate_mouse_move(at, None, Modifiers::none());
    ui.frame();
}

/// Rests on `want` as a card would report it.
fn rest_on(ui: &mut Ui, want: Want) {
    let app = ui.app.clone();
    ui.cx
        .update(|_, cx| app.update(cx, |this, cx| this.hover_intent(want, true, cx)));
    ui.wait(DWELL);
}

/// Answers the newest request for page `key` with `page`.
fn answer(ui: &mut Ui, key: &str, page: Page) {
    let seq = ui
        .app
        .read_with(&ui.cx, |app, _| app.pages.states.get(key).map(|s| s.seq))
        .expect("the page was asked for");
    ui.push(Event::Page {
        key: key.to_string(),
        seq,
        result: Ok(Box::new(page)),
        cached: false,
    });
}

#[gpui_kit::test]
fn resting_on_a_card_loads_its_page_and_a_click_joins_it(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (card, item) = page_card(&mut ui, &home);
    let key = page_key(&item);

    point_at(&mut ui, &card);
    ui.wait(DWELL - Duration::from_millis(50));
    assert!(fetched(&ui.take_sent()).is_empty(), "not before it rests");
    ui.wait(Duration::from_millis(60));
    assert_eq!(fetched(&ui.take_sent()), std::slice::from_ref(&key));

    // The click opens the page and waits for that fetch, asking nothing more.
    ui.click(&card);
    let view = ui
        .app
        .read_with(&ui.cx, |app, _| app.pages.view.target().key());
    assert_eq!(view, key);
    assert!(
        fetched(&ui.take_sent()).is_empty(),
        "the click joins the fetch"
    );
    answer(&mut ui, &key, fixture("playlist"));
    let shown = ui.app.read_with(&ui.cx, |app, _| {
        app.pages.states[&key].page.is_some() && !app.pages.states[&key].loading
    });
    assert!(shown);

    // Back on Home, the card's page is fresh: resting on it asks nothing.
    ui.keys("alt-left");
    point_at(&mut ui, &card);
    ui.wait(DWELL * 2);
    assert!(
        fetched(&ui.take_sent()).is_empty(),
        "a fresh page isn't fetched again"
    );
}

#[gpui_kit::test]
fn passing_over_or_scrolling_loads_nothing(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (card, _) = page_card(&mut ui, &home);

    point_at(&mut ui, &card);
    ui.wait(Duration::from_millis(100));
    point_away(&mut ui);
    ui.wait(DWELL * 2);
    assert!(
        fetched(&ui.take_sent()).is_empty(),
        "passing over loads nothing"
    );

    point_at(&mut ui, &card);
    ui.wait(Duration::from_millis(100));
    let at = ui.find(&card).center();
    ui.cx.simulate_event(gpui_kit::ScrollWheelEvent {
        position: at,
        delta: gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-1.))),
        ..Default::default()
    });
    ui.wait(DWELL * 2);
    assert!(
        fetched(&ui.take_sent()).is_empty(),
        "scrolling loads nothing"
    );
}

#[gpui_kit::test]
fn one_hover_fetch_at_a_time_and_the_newest_waits(cx: &mut TestAppContext) {
    let (mut ui, _) = on_home(cx);
    let [a, b, c] = ["VLa", "VLb", "VLc"].map(Target::browse);

    rest_on(&mut ui, Want::Page(a.clone()));
    assert_eq!(fetched(&ui.take_sent()), [a.key()]);
    rest_on(&mut ui, Want::Page(b.clone()));
    rest_on(&mut ui, Want::Page(c.clone()));
    assert!(fetched(&ui.take_sent()).is_empty(), "one at a time");

    answer(&mut ui, &a.key(), Page::default());
    assert_eq!(
        fetched(&ui.take_sent()),
        [c.key()],
        "the newest waiting one goes"
    );
    answer(&mut ui, &c.key(), Page::default());
    assert!(
        fetched(&ui.take_sent()).is_empty(),
        "the older one was dropped"
    );

    // Each page once a session while its copy is fresh.
    rest_on(&mut ui, Want::Page(a));
    assert!(fetched(&ui.take_sent()).is_empty());
}

#[gpui_kit::test]
fn the_budget_caps_fetches_and_resolves_a_minute(cx: &mut TestAppContext) {
    let (mut ui, _) = on_home(cx);
    let mut asked = 0;
    for n in 0..FETCHES_PER_MINUTE + 5 {
        let target = Target::browse(format!("VL{n}"));
        rest_on(&mut ui, Want::Page(target.clone()));
        if !fetched(&ui.take_sent()).is_empty() {
            asked += 1;
            answer(&mut ui, &target.key(), Page::default());
        }
    }
    assert_eq!(asked, FETCHES_PER_MINUTE);

    let mut resolves = Vec::new();
    for n in 0..RESOLVES_PER_MINUTE + 5 {
        rest_on(&mut ui, Want::Song(format!("song{n}")));
        resolves.extend(prepared(&ui.take_sent()));
    }
    rest_on(&mut ui, Want::Song("song0".into()));
    resolves.extend(prepared(&ui.take_sent()));
    assert_eq!(resolves.len(), RESOLVES_PER_MINUTE, "{resolves:?}");

    // A minute on, there is room again.
    ui.wait(Duration::from_secs(60));
    rest_on(&mut ui, Want::Page(Target::browse("VLlater")));
    assert_eq!(fetched(&ui.take_sent()).len(), 1);
}

#[gpui_kit::test]
fn resting_on_play_resolves_the_first_song(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (card, item) = page_card(&mut ui, &home);
    let key = page_key(&item);
    let button = card.replace(":card:", ":card-play:");

    point_at(&mut ui, &button);
    ui.wait(DWELL);
    assert_eq!(fetched(&ui.take_sent()), std::slice::from_ref(&key));
    let playlist = fixture("playlist");
    let first = playlist
        .shelves
        .iter()
        .flat_map(|s| s.items.iter())
        .find_map(|i| i.track.as_ref())
        .expect("the playlist fixture has songs")
        .video_id
        .clone();
    answer(&mut ui, &key, playlist);
    assert_eq!(prepared(&ui.take_sent()), [first]);
}

#[gpui_kit::test]
fn the_setting_turns_it_off(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (card, _) = page_card(&mut ui, &home);
    let app = ui.app.clone();
    ui.cx
        .update(|_, cx| app.update(cx, |this, cx| this.set_prefetch(false, cx)));
    point_at(&mut ui, &card);
    ui.wait(DWELL * 2);
    assert!(fetched(&ui.take_sent()).is_empty());
}

#[gpui_kit::test]
fn a_metered_connection_prefetches_nothing(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (card, item) = page_card(&mut ui, &home);
    ui.app
        .update(&mut ui.cx, |app, _| app.pages.prefetch.set_metered(true));

    point_at(&mut ui, &card);
    ui.wait(DWELL * 2);
    assert!(fetched(&ui.take_sent()).is_empty(), "no fetch when metered");

    // Back on an unmetered connection the same rest loads the page.
    ui.app
        .update(&mut ui.cx, |app, _| app.pages.prefetch.set_metered(false));
    point_away(&mut ui);
    point_at(&mut ui, &card);
    ui.wait(DWELL * 2);
    assert_eq!(fetched(&ui.take_sent()), vec![page_key(&item)]);
}
