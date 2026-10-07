//! M29: the keyboard in menus, on page rows and in Settings. The context
//! menu moves with the arrows (round from the ends), Home/End and a first
//! letter, opens Add to playlist's submenu with → and closes it with ←;
//! Shift+F10 or the Menu key opens a focused row's menu under it and Esc
//! gives the keyboard back to the row; the account and sleep timer menus
//! answer the same keys; in Settings the arrows change chips, segments,
//! tabs and sliders, Ctrl+Tab and Ctrl+PageDown change tabs, and Space
//! turns a switch. Tab onto a card out of sight scrolls its carousel and
//! the page to it.

use encore_core::account::Edit;
use encore_core::backend::{Command, Event};
use encore_core::equalizer::Preset;
use encore_core::model::{Account, Mixes, Page, ShelfStyle, Sleep, Target};
use gpui_kit::{Modifiers, TestAppContext, point, px};

use super::home::{home_row, on_home, quick_picks};
use super::{Ui, primary};
use crate::desktop::menu::entries;
use crate::nav::LibraryTab;
use crate::settings::Category;
use crate::theme::motion::{self, Config, Reduce, TextSize};

/// The open menu's highlighted entry's label, and its submenu's.
fn highlight(ui: &mut Ui) -> (Option<&'static str>, Option<usize>) {
    ui.app.read_with(&ui.cx, |app, _| {
        let Some(menu) = &app.desktop.layers.menu else {
            return (None, None);
        };
        let label = menu
            .selected
            .and_then(|i| entries(app, &menu.subject).get(i).map(|e| e.label));
        (label, menu.sub.as_ref().and_then(|s| s.selected))
    })
}

fn menu_open(ui: &mut Ui) -> bool {
    ui.app
        .read_with(&ui.cx, |app, _| app.desktop.layers.menu.is_some())
}

#[gpui_kit::test]
fn the_menu_moves_round_jumps_and_chooses_from_the_keyboard(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (q, _) = quick_picks(&home);
    ui.right_click(&home_row(q, 2));
    ui.find("context-menu");
    assert_eq!(highlight(&mut ui).0, None, "nothing is highlighted yet");

    ui.keys("down");
    assert_eq!(highlight(&mut ui).0, Some("Play next"));
    ui.keys("up");
    assert_eq!(
        highlight(&mut ui).0,
        Some("Copy link"),
        "↑ from the top goes round"
    );
    ui.keys("home");
    assert_eq!(highlight(&mut ui).0, Some("Play next"));
    ui.keys("end");
    assert_eq!(highlight(&mut ui).0, Some("Copy link"));
    ui.keys("down");
    assert_eq!(
        highlight(&mut ui).0,
        Some("Play next"),
        "↓ from the end too"
    );

    // Type-ahead jumps to the next entry starting with the letter.
    ui.keys("s");
    assert_eq!(highlight(&mut ui).0, Some("Start radio"));
    ui.keys("a");
    assert_eq!(highlight(&mut ui).0, Some("Add to queue"));
    ui.keys("g");
    assert!(highlight(&mut ui).0.is_some_and(|l| l.starts_with("Go to")));

    // The pointer and the keys share one highlight.
    let play_next = ui.find("menu-entry:Play next").center();
    ui.cx
        .simulate_mouse_move(play_next, None, Modifiers::none());
    ui.frame();
    assert_eq!(highlight(&mut ui).0, Some("Play next"));
    ui.keys("down");
    assert_eq!(highlight(&mut ui).0, Some("Add to queue"));

    ui.take_sent();
    ui.keys("enter");
    assert!(!menu_open(&mut ui), "choosing closes the menu");
    let sent = ui.take_sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::AddToQueue(t) if t.len() == 1)),
        "Enter ran Add to queue"
    );
}

#[gpui_kit::test]
fn shift_f10_opens_a_focused_rows_menu_and_escape_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    let (q, _) = quick_picks(&home);
    // A click plays the row and puts the keyboard on it.
    ui.click(&home_row(q, 0));
    ui.take_sent();

    ui.keys("shift-f10");
    assert!(menu_open(&mut ui), "Shift+F10 opens the row's menu");
    let row = ui.find(&home_row(q, 0));
    let menu = ui.find("context-menu");
    assert!(
        (menu.top() - row.bottom()).abs() < px(6.) && (menu.right() - row.right()).abs() < px(6.),
        "the menu hangs under the row's right end: menu {menu:?}, row {row:?}"
    );

    ui.keys("escape");
    assert!(!menu_open(&mut ui), "Esc closes it");
    // The keyboard is back on the row: the Menu key opens it again.
    ui.keys("menu");
    assert!(menu_open(&mut ui), "the row has the keyboard again");
    ui.keys("escape");

    // ↓ moves to the next row, whose menu opens under it.
    ui.keys("down");
    ui.keys("shift-f10");
    let next = ui.find(&home_row(q, 1));
    let menu = ui.find("context-menu");
    assert!(
        (menu.top() - next.bottom()).abs() < px(6.),
        "↓ moved to the next row: menu {menu:?}, row {next:?}"
    );
    // Enter on the row (not the menu) plays it.
    ui.keys("escape");
    ui.press("enter");
    let sent = ui.take_sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::PlayTracks { start: 1, .. })),
        "Enter plays the focused row"
    );
}

#[gpui_kit::test]
fn tab_onto_a_card_out_of_sight_scrolls_its_carousel_and_the_page(cx: &mut TestAppContext) {
    // Reduced motion: the carousel jumps rather than glides.
    motion::set_for_test(Config {
        reduce: Reduce::Always,
        ..Config::default()
    });
    let (mut ui, home) = on_home(cx);
    let key = Target::browse("FEmusic_home").key();
    let (q, _) = quick_picks(&home);
    let carousels: Vec<usize> = home
        .shelves
        .iter()
        .enumerate()
        .filter(|(_, s)| s.style == ShelfStyle::Carousel)
        .map(|(i, _)| i)
        .collect();
    assert!(
        carousels.len() > 1,
        "the Home fixture has carousels of cards"
    );
    let page = |ui: &mut Ui| {
        ui.app.read_with(&ui.cx, |app, _| {
            let state = &app.pages.lists[&key].state;
            (state.viewport_bounds(), state.logical_scroll_top())
        })
    };

    // The keyboard starts on Quick picks' first row and tabs on through
    // the shelves.
    ui.click(&home_row(q, 0));
    ui.take_sent();
    assert_eq!(page(&mut ui).1.item_ix, 0, "Home starts at its top");
    let mut tabs = 0;
    for s in carousels {
        let carousel = |ui: &mut Ui| {
            ui.app.read_with(&ui.cx, |app, _| {
                app.pages
                    .carousels
                    .get(&(key.clone(), s))
                    .map(|h| (h.offset().x, h.bounds()))
            })
        };
        // On until a card the carousel has to bring in.
        while carousel(&mut ui).is_none_or(|(x, _)| x == px(0.)) {
            ui.keys("tab");
            tabs += 1;
            assert!(tabs < 120, "no tab moved the carousel of shelf {s}");
        }
        let (_, view) = carousel(&mut ui).expect("drawn");

        // The focused card is in sight: its menu opens at its lower left
        // corner, inside the carousel and the page.
        ui.keys("shift-f10");
        let at = ui.app.read_with(&ui.cx, |app, _| {
            app.desktop.layers.menu.as_ref().map(|m| m.at)
        });
        let at = at.expect("Shift+F10 opens the card's menu");
        let card = crate::theme::size::CARD;
        let (list, _) = page(&mut ui);
        assert!(
            at.x >= view.left() && at.x + card <= view.right(),
            "shelf {s}: the card is inside the carousel: at {at:?}, carousel {view:?}"
        );
        assert!(
            at.y > list.top() && at.y <= list.bottom(),
            "shelf {s}: the card is inside the page: at {at:?}, page {list:?}"
        );
        // Just far enough: the card's right edge rests by the carousel's.
        let room = view.right() - (at.x + card);
        assert!(
            room < px(24.),
            "shelf {s}: the carousel brought the card just into view: {room:?} beside it"
        );
        ui.keys("escape");
    }
    assert!(
        page(&mut ui).1.item_ix > 0,
        "the page scrolled down to the last carousel's cards"
    );
    motion::set_for_test(Config::default());
}

/// Signs in, and answers Library's playlists with one of the account's.
fn signed_in_with_a_playlist(ui: &mut Ui, home: &Page) {
    ui.push(Event::Account(Account::SignedIn {
        name: "Test".into(),
        photo: None,
        source: "a test".into(),
    }));
    let mut shelf = home.shelves[0].clone();
    let mut item = shelf.items[0].clone();
    item.title = "Road trip".into();
    item.editable = Some("PL-road".into());
    item.track = None;
    shelf.items = vec![item];
    let page = Page {
        shelves: vec![shelf],
        ..Page::default()
    };
    let key = LibraryTab::Playlists.target().key();
    ui.app.update(&mut ui.cx, |app, _| {
        app.ensure_page(LibraryTab::Playlists.target(), true)
    });
    let seq = ui
        .app
        .read_with(&ui.cx, |app, _| app.pages.states[&key].seq);
    ui.push(Event::Page {
        key,
        seq,
        result: Ok(Box::new(page)),
        cached: false,
    });
    ui.take_sent();
}

#[gpui_kit::test]
fn right_opens_add_to_playlist_and_left_closes_it(cx: &mut TestAppContext) {
    let (mut ui, home) = on_home(cx);
    signed_in_with_a_playlist(&mut ui, &home);
    let (q, _) = quick_picks(&home);
    ui.right_click(&home_row(q, 2));

    ui.keys("a a");
    assert_eq!(highlight(&mut ui).0, Some("Add to playlist"));
    assert!(ui.bounds("context-submenu").is_none());
    ui.keys("right");
    assert_eq!(highlight(&mut ui), (Some("Add to playlist"), Some(0)));
    ui.find("submenu-entry:Road trip");
    ui.find("submenu-entry:New playlist");
    let menu = ui.find("context-menu");
    let sub = ui.find("context-submenu");
    assert!(sub.left() > menu.center().x, "the submenu opens beside it");

    ui.keys("down");
    assert_eq!(highlight(&mut ui).1, Some(1), "the arrows move in it");
    ui.keys("left");
    assert!(ui.bounds("context-submenu").is_none(), "← closes it");
    assert_eq!(highlight(&mut ui), (Some("Add to playlist"), None));

    // Esc closes the submenu before the menu.
    ui.keys("right");
    ui.keys("escape");
    assert!(menu_open(&mut ui) && ui.bounds("context-submenu").is_none());

    ui.keys("enter");
    assert!(ui.bounds("context-submenu").is_some(), "Enter opens it too");
    ui.keys("enter");
    assert!(!menu_open(&mut ui));
    let added = ui.take_sent().into_iter().any(|c| {
        matches!(c, Command::AccountEdit { edit: Edit::Add { playlist_id, .. }, .. }
            if playlist_id == "PL-road")
    });
    assert!(added, "Enter added the song to the playlist");
}

#[gpui_kit::test]
fn the_account_and_sleep_menus_answer_the_same_keys(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    let app = ui.app.clone();
    ui.cx
        .update(|window, cx| app.update(cx, |this, cx| this.toggle_account_menu(window, cx)));
    ui.frame();
    ui.find("account-menu");
    ui.keys("down");
    ui.find("account-menu:Reconnect");
    ui.keys("s");
    ui.keys("enter");
    let settings = ui.app.read_with(&ui.cx, |app, _| app.account.settings);
    assert!(settings, "S then Enter chose Settings");
    ui.keys("escape");
    assert!(ui.bounds("account-menu").is_none());

    ui.cx
        .update(|window, cx| app.update(cx, |this, cx| this.toggle_sleep_menu(window, cx)));
    ui.frame();
    ui.find("sleep-choice:30 minutes");
    ui.keys("down down");
    ui.keys("e");
    ui.take_sent();
    ui.keys("space");
    let sent = ui.take_sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::SleepTimer(Some(Sleep::EndOfSong)))),
        "E then Space set End of song"
    );
    let open = ui.app.read_with(&ui.cx, |app, _| app.extras.sleep_open);
    assert!(!open, "and closed the menu");

    ui.cx
        .update(|window, cx| app.update(cx, |this, cx| this.toggle_sleep_menu(window, cx)));
    ui.frame();
    ui.keys("escape");
    let open = ui.app.read_with(&ui.cx, |app, _| app.extras.sleep_open);
    assert!(!open, "Esc closes the sleep timer's menu");
}

fn preset(ui: &mut Ui) -> Preset {
    ui.app.read_with(&ui.cx, |app, _| app.equalizer().preset)
}

#[gpui_kit::test]
fn settings_chips_segments_and_tabs_follow_the_arrows(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys(&primary(","));
    ui.click("settings-category:Equalizer");
    ui.click("settings-choice:Rock");
    assert_eq!(preset(&mut ui), Preset::Rock);
    ui.keys("right");
    assert_eq!(preset(&mut ui), Preset::Electronic, "→ picks the next chip");
    ui.keys("left left");
    assert_eq!(preset(&mut ui), Preset::Vocal, "the keyboard went along");

    ui.click("settings-category:Motion and lyrics");
    let tab = |ui: &mut Ui| {
        ui.app
            .read_with(&ui.cx, |app, _| app.settings.tab(Category::Motion))
    };
    ui.keys("ctrl-tab");
    assert_eq!(tab(&mut ui), 1, "Ctrl+Tab: the next tab");
    ui.keys("ctrl-tab");
    assert_eq!(tab(&mut ui), 0, "round from the last");
    ui.keys("ctrl-pagedown");
    assert_eq!(tab(&mut ui), 1);
    ui.keys("ctrl-shift-tab");
    assert_eq!(tab(&mut ui), 0);
    ui.keys("ctrl-pageup");
    assert_eq!(tab(&mut ui), 1);
    ui.click("settings-tab:Lyrics");
    ui.keys("left");
    assert_eq!(tab(&mut ui), 0, "← on a tab");
    ui.keys("right");
    assert_eq!(tab(&mut ui), 1, "→ on a tab");
    assert_eq!(
        ui.app.read_with(&ui.cx, |app, _| app.settings.category),
        Category::Motion
    );

    // A segmented choice: Lyrics' text size.
    ui.click("lyrics-size:S");
    assert_eq!(motion::config().lyrics.size, TextSize::S);
    ui.keys("right");
    assert_eq!(motion::config().lyrics.size, TextSize::M);
    ui.keys("right right right");
    assert_eq!(
        motion::config().lyrics.size,
        TextSize::XL,
        "the end stays put"
    );
    ui.keys("left");
    assert_eq!(motion::config().lyrics.size, TextSize::L);
}

#[gpui_kit::test]
fn settings_sliders_step_and_switches_turn_from_the_keyboard(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.app.update(&mut ui.cx, |app, cx| {
        app.set_mixes(
            Mixes {
                on: true,
                seconds: 6,
            },
            cx,
        )
    });
    ui.keys(&primary(","));
    ui.click("settings-category:Playback");
    let seconds = |ui: &mut Ui| {
        ui.app
            .read_with(&ui.cx, |app, _| app.player.playback.mixes.seconds)
    };
    // A click on the slider's edge, clear of the track, gives it the keyboard.
    let slider = ui.find("slider:Crossfade length");
    let edge = point(slider.left() + px(2.), slider.center().y);
    ui.cx.simulate_click(edge, Modifiers::none());
    ui.frame();
    assert_eq!(seconds(&mut ui), 6);

    ui.keys("right");
    assert_eq!(seconds(&mut ui), 7);
    ui.keys("left left");
    assert_eq!(seconds(&mut ui), 5);
    ui.keys("shift-right");
    assert_eq!(seconds(&mut ui), Mixes::LONGEST, "ten steps, kept in range");
    ui.keys("home");
    assert_eq!(seconds(&mut ui), Mixes::SHORTEST);
    ui.keys("end");
    assert_eq!(seconds(&mut ui), Mixes::LONGEST);

    // Shift+Tab goes back to Crossfade between songs' switch; Space turns it.
    ui.keys("shift-tab");
    ui.press("space");
    let on = ui
        .app
        .read_with(&ui.cx, |app, _| app.player.playback.mixes.on);
    assert!(!on, "Space turned smooth mixes off");
}

#[gpui_kit::test]
fn the_equalizer_panel_takes_tab_and_the_arrows(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys("e");
    ui.click("eq-preset:Late night");
    assert_eq!(preset(&mut ui), Preset::LateNight);
    ui.keys("left");
    assert_eq!(preset(&mut ui), Preset::Acoustic);

    // Tab from the last chip reaches the curve: ←/→ pick a band, ↑/↓ move
    // it, Shift for three decibels, 0 resets it.
    ui.click("eq-preset:Late night");
    ui.keys("tab");
    ui.keys("right right");
    let gain = |ui: &mut Ui| ui.app.read_with(&ui.cx, |app, _| app.equalizer().gains[1]);
    let before = gain(&mut ui);
    ui.keys("up");
    assert_eq!(gain(&mut ui), before + 1.);
    ui.keys("shift-down");
    assert_eq!(gain(&mut ui), before - 2.);
    ui.keys("0");
    assert_eq!(gain(&mut ui), 0.);
}
