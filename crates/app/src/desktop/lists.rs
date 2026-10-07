//! M29: the keyboard in the account menu and the sleep timer's menu, with
//! the context menu's keys (`menu_keys`): what their entries are, and
//! what choosing one does.

use encore_core::model::{Account, Sleep};
use gpui_kit::*;

use super::menu_keys::ListNav;
use crate::app::MusicApp;

/// The menu-like lists other than the context menu.
pub struct Lists {
    pub account: ListNav,
    pub sleep: ListNav,
}

impl Lists {
    pub fn new(cx: &mut App) -> Self {
        Self {
            account: ListNav::new(cx),
            sleep: ListNav::new(cx),
        }
    }
}

/// An entry of the account menu.
#[derive(Clone, Debug)]
pub enum AccountItem {
    Channel(Option<String>),
    SignIn,
    Reconnect,
    Settings,
}

/// The account menu's entries top to bottom, with their labels: the
/// channels (when there are several), Sign in while signed out,
/// Reconnect, Settings.
pub fn account_items(app: &MusicApp) -> Vec<(SharedString, AccountItem)> {
    let mut out: Vec<(SharedString, AccountItem)> = Vec::new();
    if app.account.channels.len() > 1 {
        out.extend(app.account.channels.iter().map(|c| {
            (
                SharedString::from(c.name.clone()),
                AccountItem::Channel(c.page_id.clone()),
            )
        }));
    }
    if matches!(app.account.account, Account::SignedOut { .. }) {
        out.push(("Sign in".into(), AccountItem::SignIn));
    }
    out.push(("Reconnect".into(), AccountItem::Reconnect));
    out.push(("Settings".into(), AccountItem::Settings));
    out
}

/// The sleep timer's entries: how long (one minute too with
/// `ENCORE_SHORT_SLEEP=1`, to check the fade), and Turn off while one
/// is set.
pub fn sleep_items(app: &MusicApp) -> Vec<(&'static str, Option<Sleep>)> {
    let short = std::env::var_os("ENCORE_SHORT_SLEEP")
        .is_some()
        .then_some(("1 minute", Sleep::Minutes(1)));
    let mut out: Vec<(&'static str, Option<Sleep>)> = short
        .into_iter()
        .chain(crate::views::extras::SLEEP_CHOICES)
        .map(|(label, choice)| (label, Some(choice)))
        .collect();
    if app.player.playback.sleep.is_some() {
        out.push(("Turn off", None));
    }
    out
}

/// Which list a key or the pointer acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Account,
    Sleep,
}

impl MusicApp {
    fn list(&mut self, list: List) -> &mut ListNav {
        match list {
            List::Account => &mut self.desktop.lists.account,
            List::Sleep => &mut self.desktop.lists.sleep,
        }
    }

    fn list_labels(&self, list: List) -> Vec<SharedString> {
        match list {
            List::Account => account_items(self).into_iter().map(|(l, _)| l).collect(),
            List::Sleep => sleep_items(self)
                .into_iter()
                .map(|(l, _)| l.into())
                .collect(),
        }
    }

    /// ↑/↓ (`delta`) or Home/End (`ends`: false for the first, true for
    /// the last).
    pub fn list_move(&mut self, list: List, delta: isize, cx: &mut Context<Self>) {
        let count = self.list_labels(list).len();
        self.list(list).step(delta, count);
        cx.notify();
    }

    pub fn list_ends(&mut self, list: List, last: bool, cx: &mut Context<Self>) {
        let count = self.list_labels(list).len();
        self.list(list).ends(last, count);
        cx.notify();
    }

    /// Type-ahead in a list. False when nothing starts with `letter`.
    pub fn list_letter(&mut self, list: List, letter: char, cx: &mut Context<Self>) -> bool {
        let labels = self.list_labels(list);
        let found = self
            .list(list)
            .letter(labels.iter().map(|l| l.as_ref()), letter);
        if found {
            cx.notify();
        }
        found
    }

    /// The pointer is on entry `i`.
    pub fn list_hover(&mut self, list: List, i: usize, cx: &mut Context<Self>) {
        let nav = self.list(list);
        if nav.selected != Some(i) {
            nav.selected = Some(i);
            cx.notify();
        }
    }

    /// Runs entry `i` (the highlighted one for `None`) and closes the list.
    pub fn list_choose(
        &mut self,
        list: List,
        i: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(i) = i.or(self.list(list).selected) else {
            return;
        };
        match list {
            List::Account => {
                let Some((_, item)) = account_items(self).into_iter().nth(i) else {
                    return;
                };
                self.close_account_menu(window, cx);
                match item {
                    AccountItem::Channel(page_id) => self.use_channel(page_id, cx),
                    AccountItem::SignIn => self.open_sign_in(window, cx),
                    AccountItem::Reconnect => self.reconnect(cx),
                    AccountItem::Settings => {
                        self.open_settings_at(crate::settings::Category::Account, window, cx)
                    }
                }
            }
            List::Sleep => {
                let Some((_, choice)) = sleep_items(self).into_iter().nth(i) else {
                    return;
                };
                self.sleep(choice, cx);
                self.close_panels(window, cx);
            }
        }
    }
}
