//! M24: Settings is a wide modal with categories. Ctrl+, opens it on the
//! last category used, the arrows and Ctrl+Tab change category, search
//! finds settings across categories and opens them, Esc clears the search
//! before it closes, and the settings still work from their new places.

use encore_core::backend::Command;
use encore_core::equalizer::Preset;
use gpui_kit::{TestAppContext, px};

use super::Ui;
use crate::settings::Category;

fn open(ui: &mut Ui) -> bool {
    ui.app.read_with(&ui.cx, |app, _| app.account.settings)
}

fn category(ui: &mut Ui) -> Category {
    ui.app.read_with(&ui.cx, |app, _| app.settings.category)
}

#[gpui_kit::test]
fn ctrl_comma_opens_a_wide_modal_and_escape_closes_it(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    assert!(ui.bounds("settings").is_none());

    ui.keys("ctrl-,");
    assert!(open(&mut ui), "Settings is open");
    let sheet = ui.find("settings");
    assert!(
        sheet.size.width >= px(1000.),
        "most of the window: {sheet:?}"
    );
    assert!(ui.bounds("settings-category:About").is_some());

    ui.keys("escape");
    assert!(!open(&mut ui), "Escape closes Settings");
    assert!(ui.bounds("settings").is_none());
}

#[gpui_kit::test]
fn categories_change_by_click_arrows_and_ctrl_tab(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys("ctrl-,");
    ui.click("settings-category:Equalizer");
    assert_eq!(category(&mut ui), Category::Equalizer);
    ui.keys("down");
    assert_eq!(category(&mut ui), Category::Visuals);
    ui.keys("up up");
    assert_eq!(category(&mut ui), Category::Playback);
    ui.keys("ctrl-tab");
    assert_eq!(category(&mut ui), Category::Equalizer);

    // Ctrl+, comes back to the last category used.
    ui.keys("escape");
    ui.keys("ctrl-,");
    assert_eq!(category(&mut ui), Category::Equalizer);
    assert!(ui.bounds("settings-choice:Rock").is_some());
}

#[gpui_kit::test]
fn search_filters_across_categories_and_opens_a_result(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys("ctrl-,");
    ui.keys("/");
    ui.type_text("lyrics size");
    assert!(ui.bounds("settings-hit:Text size").is_some());
    assert!(ui.bounds("settings-hit:Crossfade length").is_none());

    // Esc clears the search first, and Settings stays open.
    ui.keys("escape");
    assert!(open(&mut ui));
    assert!(ui.bounds("settings-hit:Text size").is_none());

    // The field keeps the keyboard.
    ui.type_text("blur");
    ui.click("settings-hit:Backdrop");
    let (cat, tab, query) = ui.app.read_with(&ui.cx, |app, _| {
        (
            app.settings.category,
            app.settings.tab(Category::Visuals),
            app.settings.query.clone(),
        )
    });
    assert_eq!((cat, tab, query.as_str()), (Category::Visuals, 1, ""));

    ui.keys("/");
    ui.type_text("crossfade");
    ui.keys("enter");
    assert_eq!(category(&mut ui), Category::Playback);
}

#[gpui_kit::test]
fn a_setting_still_changes_and_resets(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys("ctrl-,");
    ui.click("settings-category:Equalizer");
    ui.take_sent();
    ui.click("settings-choice:Rock");
    let preset = ui.app.read_with(&ui.cx, |app, _| app.equalizer().preset);
    assert_eq!(preset, Preset::Rock);
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::Equalizer(eq) if eq.preset == Preset::Rock))
    );

    ui.click("settings-reset");
    let preset = ui.app.read_with(&ui.cx, |app, _| app.equalizer().preset);
    assert_eq!(preset, Preset::Flat);
    assert!(
        ui.bounds("settings-reset").is_none(),
        "nothing left to reset"
    );
}

#[gpui_kit::test]
fn the_equalizer_panel_and_play_anything_open_a_category(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys("e");
    ui.click("equalizer-settings");
    assert!(open(&mut ui));
    assert_eq!(category(&mut ui), Category::Equalizer);
    ui.keys("escape");

    ui.keys("ctrl-k");
    ui.type_text("settings about");
    ui.keys("enter");
    assert!(open(&mut ui));
    assert_eq!(category(&mut ui), Category::About);
}
