//! Ctrl+, opens Settings, and Escape closes it.

use gpui_kit::TestAppContext;

use super::Ui;

#[gpui_kit::test]
fn ctrl_comma_opens_settings(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    assert!(ui.bounds("settings").is_none());

    ui.keys("ctrl-,");
    let open = ui.app.read_with(&ui.cx, |app, _| app.account.settings);
    assert!(open, "Settings is open");
    let sheet = ui.find("settings");
    assert!(sheet.size.width > gpui_kit::px(200.), "sheet {sheet:?}");

    ui.keys("escape");
    let open = ui.app.read_with(&ui.cx, |app, _| app.account.settings);
    assert!(!open, "Escape closes Settings");
    assert!(ui.bounds("settings").is_none());
}
