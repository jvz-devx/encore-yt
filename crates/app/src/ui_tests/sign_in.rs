//! M31: the browser route says at once where it can't work. On Windows
//! only Edge, Chrome and Brave keep cookies no other app can open, so with
//! none of the readable browsers installed the sheet names the other ways
//! in instead of waiting; with Firefox installed it waits and says which
//! browser it opened.

use encore_core::backend::Command;
use encore_core::browsers::{Facts, Os, classify};
use gpui_kit::TestAppContext;

use super::Ui;
use crate::sign_in::Step;

fn open_with(ui: &mut Ui, os: Os, installed: &[&str], default: &str) {
    let report = classify(Facts {
        os,
        installed: installed.iter().map(|n| (n.to_string(), None)).collect(),
        default: Some(default.into()),
    });
    let app = ui.app.clone();
    ui.cx.update(|window, cx| {
        app.update(cx, |this, cx| {
            this.open_sign_in(window, cx);
            this.sign_in.browsers = Some(report);
        })
    });
    ui.frame();
    ui.click("sign-in-route:Sign in with your browser");
}

#[gpui_kit::test]
fn windows_with_only_edge_says_so_and_offers_the_other_ways(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    open_with(&mut ui, Os::Windows, &["Microsoft Edge"], "Microsoft Edge");
    ui.find("sign-in-cant-read");
    ui.find("sign-in-route:Import a cookies file");
    ui.find("sign-in-route:Paste cookies");
    assert!(
        ui.take_sent()
            .iter()
            .all(|c| !matches!(c, Command::ScanBrowsers)),
        "nothing to wait for"
    );
    let step = ui
        .app
        .read_with(&ui.cx, |app, _| matches!(app.sign_in.step, Step::CantRead));
    assert!(step);

    // Paste cookies is one click away.
    ui.click("sign-in-route:Paste cookies");
    assert!(ui.bounds("sign-in-cant-read").is_none());
}

#[gpui_kit::test]
fn an_unreadable_default_with_firefox_installed_waits_and_says_why(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    open_with(
        &mut ui,
        Os::Windows,
        &["Microsoft Edge", "Firefox"],
        "Microsoft Edge",
    );
    assert!(ui.bounds("sign-in-cant-read").is_none());
    let note = ui.app.read_with(&ui.cx, |app, _| match &app.sign_in.step {
        Step::Waiting { note, .. } => note.clone(),
        _ => None,
    });
    assert_eq!(
        note.as_deref(),
        Some("Opening Firefox, because Edge can't be read.")
    );
    assert!(
        ui.take_sent()
            .iter()
            .any(|c| matches!(c, Command::ScanBrowsers))
    );
}

#[gpui_kit::test]
fn readable_browsers_wait_as_before(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    open_with(&mut ui, Os::Linux, &["Google Chrome"], "Google Chrome");
    let note = ui.app.read_with(&ui.cx, |app, _| match &app.sign_in.step {
        Step::Waiting { note, .. } => Some(note.clone()),
        _ => None,
    });
    assert_eq!(note, Some(None));
}
