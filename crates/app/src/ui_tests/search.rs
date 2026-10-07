//! Typing in search asks for suggestions once typing pauses; Enter opens
//! the search's page.

use std::time::Duration;

use gpui_kit::TestAppContext;
use ytfast::backend::{Command, Event};
use ytfast::model::Target;

use super::Ui;
use crate::nav::View;

fn suggested(sent: &[Command]) -> Vec<&str> {
    sent.iter()
        .filter_map(|c| match c {
            Command::Suggest(text) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[gpui_kit::test]
fn typing_asks_for_suggestions_and_enter_opens_the_search(cx: &mut TestAppContext) {
    let mut ui = Ui::start(cx);
    ui.keys("ctrl-f");
    ui.take_sent();

    ui.type_text("daft");
    assert!(
        suggested(&ui.take_sent()).is_empty(),
        "nothing is asked while typing"
    );
    ui.wait(Duration::from_millis(100));
    ui.type_text(" p");
    ui.wait(Duration::from_millis(100));
    assert!(suggested(&ui.take_sent()).is_empty(), "typing put it off");
    ui.wait(Duration::from_millis(100));
    assert_eq!(suggested(&ui.take_sent()), ["daft p"]);

    ui.push(Event::Suggestions {
        input: "daft p".into(),
        items: vec!["daft punk".into(), "daft punk get lucky".into()],
    });
    ui.find("suggestion:daft punk");
    ui.find("suggestion:daft punk get lucky");

    ui.keys("enter");
    let view = ui.app.read_with(&ui.cx, |app, _| app.pages.view.clone());
    let search = Target::Search {
        query: "daft p".into(),
        params: None,
    };
    assert_eq!(view, View::Page(search.clone()));
    let sent = ui.take_sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::Page { target, .. } if target.key() == search.key())),
        "the search page is asked for"
    );
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::SaveSearches(list) if list.first().map(String::as_str) == Some("daft p"))),
        "the search is remembered"
    );
    assert!(
        ui.bounds("suggestion:daft punk").is_none(),
        "the list closed"
    );
}
