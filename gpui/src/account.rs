//! M3: the signed-in account, browser profiles, and changes to the account.

use gpui_kit::*;
use ytfast::account::{Done, Failure};
use ytfast::auth::Profile;
use ytfast::model::{Account, LikeStatus, Target};

use crate::app::MusicApp;

pub struct AccountUi {
    pub account: Account,
    pub profiles: Vec<Profile>,
    pub profile: Option<String>,
}

impl AccountUi {
    pub fn new(_window: &mut Window, _cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        (
            Self {
                account: Account::Checking,
                profiles: Vec::new(),
                profile: None,
            },
            Vec::new(),
        )
    }

    pub fn signed_in(&self) -> bool {
        matches!(self.account, Account::SignedIn { .. })
    }
}

impl MusicApp {
    pub(crate) fn on_account(&mut self, account: Account, _cx: &mut Context<Self>) {
        let was = self.account.signed_in();
        // The state only, never the name: scripts/gpui-smoke.sh checks it.
        log::info!(
            "account: {}",
            match &account {
                Account::Checking => "checking",
                Account::SignedIn { .. } => "signed in",
                Account::SignedOut { .. } => "signed out",
                Account::Unverified { .. } => "unverified",
            }
        );
        self.account.account = account;
        if was != self.account.signed_in() {
            // Home, Library and the rest change with the account.
            self.ensure_page(self.pages.view.target(), true);
        }
        self.sidebar_account(was != self.account.signed_in());
    }

    pub(crate) fn on_profiles(&mut self, list: Vec<Profile>, current: Option<String>) {
        self.account.profiles = list;
        self.account.profile = current;
    }

    pub(crate) fn on_account_edited(
        &mut self,
        _op: u64,
        _result: Result<Done, Failure>,
        _cx: &mut Context<Self>,
    ) {
        // M3: optimistic account edits.
    }

    pub(crate) fn on_likes(&mut self, _likes: Vec<(String, LikeStatus)>) {
        // M3: ratings.
    }

    pub(crate) fn on_account_refresh(&mut self, targets: Vec<Target>) {
        for target in targets {
            self.ensure_page(target, true);
        }
    }
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(_cx: &mut App) {}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, _cx: &mut Context<MusicApp>) -> Div {
    root
}
