//! M24: where Settings is: the category showing (and the one Ctrl+, comes
//! back to), each category's tab, and the search over every setting.
//! `views::settings` draws the modal; `AccountUi::settings` says whether it
//! is open.
//!
//! Keys inside it: ↑/↓ (or Ctrl+Tab, Ctrl+Shift+Tab) change the category,
//! `/` goes to the search field, Esc clears the search and then closes.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::app::MusicApp;

mod index;

pub use index::{Entry, search};

actions!(settings, [NextCategory, PreviousCategory, FocusSearch]);

/// The key context of the Settings modal (inside `MusicDialog`).
pub const CONTEXT: &str = "Settings";

/// A category in Settings' sidebar, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Account,
    Playback,
    Equalizer,
    Visuals,
    Motion,
    Shortcuts,
    Updates,
    About,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Account,
        Category::Playback,
        Category::Equalizer,
        Category::Visuals,
        Category::Motion,
        Category::Shortcuts,
        Category::Updates,
        Category::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Account => "Account",
            Category::Playback => "Playback",
            Category::Equalizer => "Equalizer",
            Category::Visuals => "Visuals",
            Category::Motion => "Motion and lyrics",
            Category::Shortcuts => "Keyboard shortcuts",
            Category::Updates => "Updates",
            Category::About => "About",
        }
    }

    /// One line under the category's title.
    pub fn blurb(self) -> &'static str {
        match self {
            Category::Account => "Who is signed in, and the YouTube session Music uses",
            Category::Playback => {
                "Loudness, smooth mixes, the sleep timer, notifications and loading ahead"
            }
            Category::Equalizer => "Shape the sound of every song",
            Category::Visuals => "Backdrops, glows and the visualiser that move with the music",
            Category::Motion => "How things move, and how timed lyrics glide",
            Category::Shortcuts => "Every key Music answers",
            Category::Updates => "New versions from GitHub Releases",
            Category::About => "Version, credits, and where Music keeps its files",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Category::Account => IconName::CircleUserRound,
            Category::Playback => IconName::CirclePlay,
            Category::Equalizer => IconName::SlidersVertical,
            Category::Visuals => IconName::Sparkles,
            Category::Motion => IconName::Wind,
            Category::Shortcuts => IconName::Keyboard,
            Category::Updates => IconName::CircleArrowUp,
            Category::About => IconName::Info,
        }
    }

    /// The tabs inside the category, if it has any.
    pub fn tabs(self) -> &'static [&'static str] {
        match self {
            Category::Visuals => &[
                "General",
                "Backdrop",
                "Particles",
                "Player bar",
                "Visualiser",
                "Transitions",
            ],
            Category::Motion => &["Motion", "Lyrics"],
            // `Group::ALL`'s titles.
            Category::Shortcuts => &["Playback", "Library", "Navigation", "Views", "In Settings"],
            _ => &[],
        }
    }

    fn index(self) -> usize {
        Category::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// The category `step` places down the list (up when negative),
    /// wrapping round.
    fn step(self, step: isize) -> Category {
        let n = Category::ALL.len() as isize;
        Category::ALL[(self.index() as isize + step).rem_euclid(n) as usize]
    }
}

pub struct SettingsNav {
    /// The category showing, and the one Settings opens on next.
    pub category: Category,
    tabs: [usize; Category::ALL.len()],
    /// The search field at the top of the sidebar, made afresh each time
    /// Settings opens, in the window showing it.
    pub search: Option<Entity<InputState>>,
    _search: Option<Subscription>,
    /// What is typed in it, trimmed.
    pub query: String,
    /// The highlighted search result.
    pub hit: usize,
    /// About shows the third-party notices.
    pub notices: bool,
    /// The sidebar's items, so ↑/↓ can carry the keyboard focus along.
    pub items: Vec<FocusHandle>,
}

impl SettingsNav {
    pub fn new(cx: &mut Context<MusicApp>) -> Self {
        Self {
            category: Category::Account,
            tabs: [0; Category::ALL.len()],
            search: None,
            _search: None,
            query: String::new(),
            hit: 0,
            notices: false,
            items: Category::ALL.iter().map(|_| cx.focus_handle()).collect(),
        }
    }

    /// The tab showing in `category` (0 when it has none).
    pub fn tab(&self, category: Category) -> usize {
        self.tabs[category.index()]
    }

    pub fn set_tab(&mut self, category: Category, tab: usize) {
        self.tabs[category.index()] = tab;
    }

    pub fn item(&self, category: Category) -> &FocusHandle {
        &self.items[category.index()]
    }
}

pub fn bind_keys(cx: &mut App) {
    let keys = Some("Settings && !Input");
    let anywhere = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("down", NextCategory, keys),
        KeyBinding::new("up", PreviousCategory, keys),
        KeyBinding::new("/", FocusSearch, keys),
        KeyBinding::new("ctrl-tab", NextCategory, anywhere),
        KeyBinding::new("ctrl-shift-tab", PreviousCategory, anywhere),
    ]);
}

impl MusicApp {
    /// A fresh, empty search field for Settings opening in `window`.
    pub(crate) fn prepare_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        self.settings._search = Some(cx.subscribe_in(&search, window, Self::on_settings_search));
        self.settings.search = Some(search);
        self.settings.query.clear();
        self.settings.hit = 0;
    }

    /// Opens Settings on `category`.
    pub fn open_settings_at(
        &mut self,
        category: Category,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.category = category;
        self.open_settings(true, window, cx);
    }

    /// Shows `category` (a click in the sidebar), leaving a search.
    pub fn show_category(
        &mut self,
        category: Category,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_settings_search(window, cx);
        self.settings.category = category;
        cx.notify();
    }

    /// Shows a category and tab from a search result.
    pub fn jump_to_setting(
        &mut self,
        category: Category,
        tab: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = tab {
            self.settings.set_tab(category, tab);
        }
        self.show_category(category, window, cx);
        self.settings.item(category).clone().focus(window, cx);
    }

    /// ↑/↓: the next category up or down; the keyboard focus follows when
    /// it was on the sidebar.
    pub fn step_category(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let on_sidebar = self.settings.items.iter().any(|f| f.is_focused(window));
        let next = self.settings.category.step(step);
        self.show_category(next, window, cx);
        if on_sidebar {
            self.settings.item(next).clone().focus(window, cx);
        }
    }

    pub fn focus_settings_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = self.settings.search.clone() {
            search.update(cx, |input, cx| input.focus(window, cx));
            cx.notify();
        }
    }

    /// Empties the search field. False when it was empty already.
    pub fn clear_settings_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.settings.query.is_empty() {
            return false;
        }
        self.settings.query.clear();
        self.settings.hit = 0;
        if let Some(search) = self.settings.search.clone() {
            search.update(cx, |input, cx| input.set_value("", window, cx));
        }
        cx.notify();
        true
    }

    /// Esc in Settings: the search first, then Settings itself.
    pub fn settings_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.clear_settings_search(window, cx) {
            self.open_settings(false, window, cx);
        }
    }

    /// Moves the highlighted search result.
    pub fn move_settings_hit(&mut self, step: isize, count: usize, cx: &mut Context<Self>) {
        if count > 0 {
            let at = self.settings.hit as isize + step;
            self.settings.hit = at.clamp(0, count as isize - 1) as usize;
            cx.notify();
        }
    }

    fn on_settings_search(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.settings.query = input.read(cx).value().trim().to_string();
                self.settings.hit = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                let hits = search(&self.settings.query);
                if let Some(hit) = hits.get(self.settings.hit) {
                    self.jump_to_setting(hit.category, hit.tab, window, cx);
                }
            }
            _ => {}
        }
    }
}
