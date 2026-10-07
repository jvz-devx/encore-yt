//! M3: the signed-in account, browser profiles, Settings, and changes to
//! the account; M12: the account's channels.
//!
//! The optimistic logic is the backend crate's [`AccountState`] (shared with
//! the egui app): this module lends it the page cache and the backend
//! through a [`Host`] and carries out the [`Effects`] it returns. Views draw
//! from [`AccountState::marks`] and open dialogs through the methods here;
//! `views::account` and `views::settings` draw them.

use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use ytfast::account::{
    AccountAction, AccountState, CachedPage, Dialog, Done, Effects, Failure, Host, playlist_keys,
};
use ytfast::auth::Profile;
use ytfast::backend::Command;
use ytfast::model::{Account, Channel, LikeStatus, Page, Target, Track};

use crate::app::MusicApp;
use crate::nav::{LibraryTab, PageState, View};

/// The key context of an account dialog and of Settings: single-key
/// shortcuts stay out of them.
pub const DIALOG_CONTEXT: &str = "MusicDialog";

/// A click on the chip right after the menu closed by that same press
/// doesn't open it again.
const REOPEN_GUARD: Duration = Duration::from_millis(150);

impl CachedPage for PageState {
    fn page(&self) -> Option<&Page> {
        self.page.as_ref()
    }

    fn page_mut(&mut self) -> Option<&mut Page> {
        self.page.as_mut()
    }

    fn target(&self) -> &Target {
        &self.target
    }

    fn start_more(&mut self, shelf: usize) -> bool {
        self.more_loading.insert(Some(shelf))
    }
}

pub struct AccountUi {
    pub account: Account,
    pub profiles: Vec<Profile>,
    pub profile: Option<String>,
    /// The account's YouTube channels, the one requests act as `current`.
    pub channels: Vec<Channel>,
    /// Likes, library state, subscriptions as shown, and the open dialog.
    pub state: AccountState,
    /// The account menu under the chip is open.
    pub menu: bool,
    menu_closed: Option<Instant>,
    /// Settings is open over the window.
    pub settings: bool,
    /// The open dialog's text fields, made in the window that shows it.
    pub fields: Option<Fields>,
    /// Focus for dialogs and Settings, so Escape and Enter reach them.
    pub focus: FocusHandle,
}

/// One of the account's playlists, offered by Add to playlist.
pub struct PlaylistChoice {
    pub id: String,
    pub title: String,
    pub thumbnail: Option<String>,
}

/// The text fields of a dialog: a name (or the playlist filter) and a
/// description.
pub struct Fields {
    pub first: Entity<InputState>,
    pub second: Option<Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
}

impl AccountUi {
    pub fn new(_window: &mut Window, cx: &mut Context<MusicApp>) -> (Self, Vec<Subscription>) {
        (
            Self {
                account: Account::Checking,
                profiles: Vec::new(),
                profile: None,
                channels: Vec::new(),
                state: AccountState::default(),
                menu: false,
                menu_closed: None,
                settings: false,
                fields: None,
                focus: cx.focus_handle(),
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
        if !matches!(account, Account::Checking) {
            crate::startup::mark(crate::startup::Milestone::AccountChecked);
        }
        self.account.account = account;
        if self.account.signed_in() && !was {
            // Pages fetched before the session was confirmed may be public
            // copies; Library's playlists feed Add to playlist.
            self.ensure_page(self.pages.view.target(), true);
            self.ensure_page(LibraryTab::Playlists.target(), true);
            // A song restored from the last session has no rating yet: only
            // a song that starts playing brings its own.
            if let Some(track) = self.player.current() {
                self.send(Command::LikeStatus(track.video_id.clone()));
            }
            if self.pages.view != View::Home {
                self.ensure_page(View::Home.target(), true);
            }
        } else if was && !self.account.signed_in() {
            self.ensure_page(self.pages.view.target(), true);
        }
        self.sidebar_account();
    }

    pub(crate) fn on_profiles(&mut self, list: Vec<Profile>, current: Option<String>) {
        self.account.profiles = list;
        self.account.profile = current;
    }

    pub(crate) fn on_channels(&mut self, channels: Vec<Channel>) {
        self.account.channels = channels;
    }

    pub(crate) fn on_account_edited(
        &mut self,
        op: u64,
        result: Result<Done, Failure>,
        _cx: &mut Context<Self>,
    ) {
        self.with_account(|state, host| state.edited(host, op, result));
    }

    pub(crate) fn on_likes(&mut self, likes: Vec<(String, LikeStatus)>) {
        log::info!("ratings from YouTube Music: {likes:?}");
        self.account.state.likes_fetched(likes);
    }

    pub(crate) fn on_account_refresh(&mut self, targets: Vec<Target>) {
        let effects = AccountState::refresh(&self.pages.states, targets);
        self.carry_out(effects);
    }

    /// A page (saved or fresh) is in the cache: M1's `on_page` calls this
    /// after storing it.
    pub(crate) fn account_page_arrived(&mut self, key: &str, cached: bool) {
        self.with_account(|state, host| state.page_arrived(host, key, cached));
    }

    /// More rows of a page are in the cache: M1's `on_more` calls this after
    /// merging them.
    pub(crate) fn account_more_arrived(&mut self, key: &str) {
        self.with_account(|state, host| state.more_arrived(host, key));
    }

    /// Runs `call` with this app as the host, then carries out its effects.
    fn with_account(
        &mut self,
        call: impl FnOnce(&mut AccountState, &mut Host<'_, PageState>) -> Effects,
    ) {
        let signed_in = self.account.signed_in();
        let backend = &self.backend;
        let send = |command| backend.send(command);
        let mut host = Host {
            pages: &mut self.pages.states,
            send: &send,
            signed_in,
        };
        let effects = call(&mut self.account.state, &mut host);
        self.carry_out(effects);
    }

    fn carry_out(&mut self, effects: Effects) {
        for target in effects.refetch {
            self.ensure_page(target, true);
        }
        // One strip: the newest message stands.
        if let Some(message) = effects.messages.into_iter().last() {
            self.error = Some(message);
        }
    }

    /// Makes a change to the account: shown at once, rolled back with a
    /// message if YouTube Music refuses.
    pub fn account_act(&mut self, action: AccountAction, cx: &mut Context<Self>) {
        let signed_in = self.account.signed_in();
        // Disliking the playing song moves on, as YouTube Music does.
        let skip = signed_in
            && matches!(&action, AccountAction::Rate { track, status: LikeStatus::Dislike }
                if self.player.current().is_some_and(|t| t.video_id == track.video_id));
        if let AccountAction::Delete { playlist_id } = &action
            && signed_in
        {
            self.leave_playlist(playlist_id);
        }
        self.with_account(|state, host| state.act(host, action));
        if skip {
            self.send(Command::Next);
        }
        cx.notify();
    }

    /// Leaves the page of a playlist about to be deleted, without leaving a
    /// way Forward to it.
    fn leave_playlist(&mut self, playlist_id: &str) {
        let keys = playlist_keys(&self.pages.states, playlist_id);
        if matches!(&self.pages.view, View::Page(t) if keys.contains(&t.key())) {
            self.pages.view = self
                .pages
                .history
                .pop()
                .unwrap_or(View::Library(LibraryTab::Playlists));
            self.ensure_page(self.pages.view.target(), false);
        }
        self.pages
            .history
            .retain(|v| !matches!(v, View::Page(t) if keys.contains(&t.key())));
        self.pages.forward.clear();
    }

    /// Likes the playing song, or removes its like (command line, keys).
    pub fn toggle_like_current(&mut self, cx: &mut Context<Self>) {
        let Some(track) = self.player.current().cloned() else {
            self.notice("Nothing is playing", cx);
            return;
        };
        let status = match self.account.state.marks.like(&track) {
            LikeStatus::Like => LikeStatus::Indifferent,
            _ => LikeStatus::Like,
        };
        self.account_act(AccountAction::Rate { track, status }, cx);
    }

    /// Opens a dialog over the window, with its fields focused.
    pub fn open_account_dialog(
        &mut self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.account.menu = false;
        self.account.settings = false;
        if matches!(dialog, Dialog::AddToPlaylist { .. }) {
            // Add to playlist lists Library's playlists; keep them fresh.
            self.ensure_page(LibraryTab::Playlists.target(), false);
        }
        self.account.fields = fields_for(&dialog, window, cx);
        match &self.account.fields {
            Some(fields) => fields.first.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            }),
            None => self.account.focus.focus(window, cx),
        }
        self.account.state.dialog = Some(dialog);
        cx.notify();
    }

    pub fn close_account_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.account.state.dialog = None;
        self.account.fields = None;
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// The dialog's confirming button (or Enter): does what it says.
    pub fn confirm_account_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.account.state.dialog.clone() else {
            return;
        };
        let text = |field: Option<&Entity<InputState>>, cx: &App| {
            field.map(|f| f.read(cx).value().trim().to_string())
        };
        let first = text(self.account.fields.as_ref().map(|f| &f.first), cx).unwrap_or_default();
        let second = text(
            self.account.fields.as_ref().and_then(|f| f.second.as_ref()),
            cx,
        )
        .unwrap_or_default();
        let action = match dialog {
            Dialog::NewPlaylist { tracks, .. } if !first.is_empty() => AccountAction::Create {
                title: first,
                description: second,
                tracks,
            },
            Dialog::EditPlaylist { playlist_id, .. } if !first.is_empty() => {
                AccountAction::Details {
                    playlist_id,
                    title: first,
                    description: second,
                }
            }
            Dialog::DeletePlaylist { playlist_id, .. } => AccountAction::Delete { playlist_id },
            Dialog::AddToPlaylist {
                tracks, selected, ..
            } => match self.playlist_matches().unwrap_or_default().get(selected) {
                Some(choice) => AccountAction::Add {
                    playlist_id: choice.id.clone(),
                    tracks,
                },
                None => return,
            },
            _ => return,
        };
        self.close_account_dialog(window, cx);
        self.account_act(action, cx);
    }

    /// Add to playlist: the account's playlists matching the filter, from
    /// Library. `None` while Library's playlists haven't loaded.
    pub fn playlist_matches(&self) -> Option<Vec<PlaylistChoice>> {
        let filter = match &self.account.state.dialog {
            Some(Dialog::AddToPlaylist { filter, .. }) => filter.trim().to_lowercase(),
            _ => String::new(),
        };
        let library = self
            .pages
            .states
            .get(&LibraryTab::Playlists.target().key())
            .and_then(|s| s.page.as_ref())?;
        Some(
            library
                .shelves
                .iter()
                .flat_map(|s| &s.items)
                .filter_map(|i| {
                    Some(PlaylistChoice {
                        id: i.editable.clone()?,
                        title: i.title.clone(),
                        thumbnail: i.thumbnail.clone(),
                    })
                })
                .filter(|p| filter.is_empty() || p.title.to_lowercase().contains(&filter))
                .collect(),
        )
    }

    /// Add to playlist: moves the highlighted playlist up or down.
    pub fn move_playlist_selection(&mut self, down: bool, cx: &mut Context<Self>) {
        let count = self.playlist_matches().map_or(0, |m| m.len());
        if let Some(Dialog::AddToPlaylist { selected, .. }) = &mut self.account.state.dialog {
            *selected = if down {
                (*selected + 1).min(count.saturating_sub(1))
            } else {
                selected.saturating_sub(1)
            };
            cx.notify();
        }
    }

    /// Escape: closes the account's dialog, else Settings, else the account
    /// menu. False when none is open.
    pub fn close_account_layer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.sign_in.open {
            self.close_sign_in(window, cx);
        } else if self.account.state.dialog.is_some() {
            self.close_account_dialog(window, cx);
        } else if self.account.settings {
            self.settings_escape(window, cx);
        } else if self.account.menu {
            self.close_account_menu(window, cx);
        } else {
            return false;
        }
        true
    }

    /// The account chip: opens its menu, which takes the keyboard, or
    /// closes it.
    pub fn toggle_account_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.account.menu {
            self.close_account_menu(window, cx);
            return;
        }
        let just_closed = self
            .account
            .menu_closed
            .is_some_and(|t| t.elapsed() < REOPEN_GUARD);
        if !just_closed {
            self.account.menu = true;
            self.desktop.lists.account.open(window, cx);
        }
        cx.notify();
    }

    /// Closes the account menu; the keyboard goes back to where it was.
    pub fn close_account_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.account.menu {
            self.account.menu = false;
            self.account.menu_closed = Some(Instant::now());
            self.desktop.lists.account.close(&self.focus, window, cx);
            cx.notify();
        }
    }

    pub fn open_settings(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.account.menu = false;
        if open && !self.account.settings {
            self.prepare_settings(window, cx);
        }
        self.account.settings = open;
        if open {
            self.account.focus.focus(window, cx);
        } else {
            self.focus.focus(window, cx);
        }
        cx.notify();
    }

    pub fn reconnect(&mut self, cx: &mut Context<Self>) {
        self.account.menu = false;
        self.account.account = Account::Checking;
        self.send(Command::Reconnect);
        cx.notify();
    }

    pub fn use_profile(&mut self, id: String, cx: &mut Context<Self>) {
        if self.account.profile.as_deref() != Some(id.as_str()) {
            self.account.profile = Some(id.clone());
            self.account.account = Account::Checking;
            self.send(Command::UseProfile(id));
            cx.notify();
        }
    }

    /// Acts as another of the account's channels (`page_id` from
    /// [`Channel::page_id`]). The backend reconnects; once the account is
    /// confirmed again, Home, Library and the page shown are fetched anew
    /// (see `on_account`), and every other page on its next visit.
    pub fn use_channel(&mut self, page_id: Option<String>, cx: &mut Context<Self>) {
        self.account.menu = false;
        let current = self.account.channels.iter().find(|c| c.current);
        if current.is_none_or(|c| c.page_id != page_id) {
            for channel in &mut self.account.channels {
                channel.current = channel.page_id == page_id;
            }
            for state in self.pages.states.values_mut() {
                state.fetched = None;
            }
            self.account.account = Account::Checking;
            self.send(Command::UseChannel(page_id));
        }
        cx.notify();
    }

    /// Settings: song-change notifications (read by the desktop module).
    pub fn notifications(&self) -> bool {
        self.desktop
            .flags
            .notifications
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_notifications(&mut self, on: bool, cx: &mut Context<Self>) {
        self.desktop
            .flags
            .notifications
            .store(on, std::sync::atomic::Ordering::Relaxed);
        self.send(Command::Notifications(on));
        cx.notify();
    }
}

/// Text fields for `dialog`, made in `window`: a name and a description,
/// or the playlist filter. `None` for a dialog without fields.
fn fields_for(dialog: &Dialog, window: &mut Window, cx: &mut Context<MusicApp>) -> Option<Fields> {
    let field = |text: &str, placeholder: &str, window: &mut Window, cx: &mut Context<MusicApp>| {
        let (text, placeholder) = (text.to_string(), placeholder.to_string());
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(text)
        })
    };
    match dialog {
        Dialog::NewPlaylist {
            title, description, ..
        }
        | Dialog::EditPlaylist {
            title, description, ..
        } => Some(Fields {
            first: field(title, "Playlist name", window, cx),
            second: Some(field(description, "Description", window, cx)),
            _subscriptions: Vec::new(),
        }),
        Dialog::AddToPlaylist { filter, .. } => {
            let first = field(filter, "Find a playlist", window, cx);
            let subscription =
                cx.subscribe_in(&first, window, |this, input, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        let value = input.read(cx).value().to_string();
                        if let Some(Dialog::AddToPlaylist {
                            filter, selected, ..
                        }) = &mut this.account.state.dialog
                        {
                            *filter = value;
                            *selected = 0;
                        }
                        cx.notify();
                    }
                });
            Some(Fields {
                first,
                second: None,
                _subscriptions: vec![subscription],
            })
        }
        Dialog::DeletePlaylist { .. } => None,
    }
}

impl MusicApp {
    /// Up next's Save: New playlist holding the queue, each song once.
    pub fn save_queue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut seen = std::collections::HashSet::new();
        let tracks: Vec<Track> = self
            .player
            .queue
            .iter()
            .filter(|t| seen.insert(t.video_id.clone()))
            .cloned()
            .collect();
        if tracks.is_empty() {
            return;
        }
        let dialog = Dialog::NewPlaylist {
            title: format!("Queue · {}", today()),
            description: String::new(),
            tracks,
        };
        self.open_account_dialog(dialog, window, cx);
    }
}

/// Today's date as "7 Oct 2026", for a saved queue's name.
fn today() -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

/// Opens Add to playlist for `tracks` (the player bar, rows, and later
/// context menus).
pub fn add_to_playlist(
    app: &mut MusicApp,
    tracks: Vec<Track>,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) {
    app.open_account_dialog(
        Dialog::AddToPlaylist {
            tracks,
            filter: String::new(),
            selected: 0,
        },
        window,
        cx,
    );
}

/// Shortcuts for this area, in the "Music" key context.
pub fn bind_keys(_cx: &mut App) {}

/// Handlers for this area's actions, on the window's root element.
pub fn on_actions(root: Div, _cx: &mut Context<MusicApp>) -> Div {
    root
}
