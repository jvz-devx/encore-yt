//! Search: the field's text, suggestions as you type, recent searches, and
//! running a search.
//!
//! The field's `InputState` belongs to the window it was made in, and the
//! window can close while the app lives on (desktop). So the field is made
//! per window: [`MusicApp::search_input`] makes a new one, with the same
//! text, the first time a new window draws it.

use std::time::Duration;

use encore_core::backend::Command;
use encore_core::model::Target;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use crate::app::MusicApp;
use crate::nav::View;

/// Typing pauses this long before suggestions are asked for.
const SUGGEST_DELAY: Duration = Duration::from_millis(150);
/// Suggestions shown under the field.
pub const SUGGESTIONS: usize = 8;
/// Recent searches shown under the empty field.
pub const RECENT: usize = 8;

const PLACEHOLDER: &str = "Search songs, albums, artists, podcasts";

/// The search field of one window.
pub struct SearchField {
    pub input: Entity<InputState>,
    window: WindowId,
    _subscriptions: Vec<Subscription>,
}

/// What the list under the field shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropdown {
    /// The field is empty: recent searches.
    Recent,
    /// Suggestions for the text typed.
    Suggestions,
}

#[derive(Default)]
pub struct Search {
    pub field: Option<SearchField>,
    /// The field's text as typed.
    pub value: String,
    pub focused: bool,
    pub suggestions: Vec<String>,
    /// The text suggestions were last asked for.
    suggested_for: String,
    /// Recent searches, newest first.
    pub recent: Vec<String>,
    /// The suggestion or recent search picked with the arrow keys.
    pub highlight: Option<usize>,
    /// Escape (or a search) closed the list until the text changes.
    pub closed: bool,
    loaded: bool,
    debounce: Option<Task<()>>,
}

impl Search {
    /// The field's text, trimmed.
    pub fn text(&self) -> &str {
        self.value.trim()
    }

    /// What the list under the field shows now, if anything.
    pub fn dropdown(&self) -> Option<Dropdown> {
        if !self.focused || self.closed {
            None
        } else if self.text().is_empty() {
            (!self.recent.is_empty()).then_some(Dropdown::Recent)
        } else {
            (!self.suggestions.is_empty()).then_some(Dropdown::Suggestions)
        }
    }

    /// The entries of the list under the field.
    pub fn entries(&self) -> &[String] {
        match self.dropdown() {
            Some(Dropdown::Recent) => &self.recent[..self.recent.len().min(RECENT)],
            Some(Dropdown::Suggestions) => {
                &self.suggestions[..self.suggestions.len().min(SUGGESTIONS)]
            }
            None => &[],
        }
    }
}

impl MusicApp {
    /// This window's search field, made the first time a window draws it.
    pub fn search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        let id = window.window_handle().window_id();
        if let Some(field) = &self.pages.search.field
            && field.window == id
        {
            return field.input.clone();
        }
        let value = self.pages.search.value.clone();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder(PLACEHOLDER);
            if !value.is_empty() {
                state.set_value(value, window, cx);
            }
            state
        });
        let subscription = cx.subscribe_in(&input, window, Self::on_search_input);
        if self.pages.search.field.is_some() {
            log::info!("search field made again for a new window");
        }
        self.pages.search.field = Some(SearchField {
            input: input.clone(),
            window: id,
            _subscriptions: vec![subscription],
        });
        self.pages.search.focused = false;
        if !std::mem::replace(&mut self.pages.search.loaded, true) {
            self.send(Command::LoadSearches);
        }
        input
    }

    fn on_search_input(
        &mut self,
        state: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let search = &mut self.pages.search;
        match event {
            InputEvent::Change => {
                search.value = state.read(cx).value().to_string();
                search.closed = false;
                search.highlight = None;
                self.suggest_later(cx);
            }
            InputEvent::PressEnter { .. } => {
                let query = search
                    .highlight
                    .and_then(|i| search.entries().get(i).cloned())
                    .unwrap_or_else(|| search.text().to_string());
                self.run_search(query, window, cx);
            }
            InputEvent::Focus => {
                search.focused = true;
                search.closed = false;
            }
            InputEvent::Blur => {
                search.focused = false;
                search.highlight = None;
            }
        }
        cx.notify();
    }

    /// Asks for suggestions once typing pauses.
    fn suggest_later(&mut self, cx: &mut Context<Self>) {
        let search = &mut self.pages.search;
        let input = search.text().to_string();
        if input == search.suggested_for {
            return;
        }
        if input.chars().count() < 2 {
            search.suggestions.clear();
            search.suggested_for = input;
            search.debounce = None;
            return;
        }
        search.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SUGGEST_DELAY).await;
            let _ = this.update(cx, |this, _| {
                let search = &mut this.pages.search;
                if search.text() == input && search.suggested_for != input {
                    search.suggested_for = input.clone();
                    log::info!("asking for suggestions for {input:?}");
                    this.send(Command::Suggest(input));
                }
            });
        }));
    }

    /// Runs a search for `query` (or opens a pasted link), remembers it,
    /// and closes the list.
    pub fn run_search(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        let query = query.trim().to_string();
        if query.is_empty() {
            return;
        }
        let search = &mut self.pages.search;
        search.suggestions.clear();
        search.highlight = None;
        search.closed = true;
        window.focus(&self.focus, cx);
        if let Some(target) = encore_core::links::target_from_link(&query) {
            // A pasted link opens what it links to.
            self.set_search_text(String::new(), window, cx);
            self.activate(target, cx);
            return;
        }
        encore_core::searches::remember(&mut self.pages.search.recent, &query);
        self.save_searches();
        self.set_search_text(query.clone(), window, cx);
        self.open(
            View::Page(Target::Search {
                query,
                params: None,
            }),
            cx,
        );
    }

    fn set_search_text(&mut self, text: String, window: &mut Window, cx: &mut Context<Self>) {
        self.pages.search.value = text.clone();
        self.pages.search.suggested_for = text.trim().to_string();
        if let Some(field) = &self.pages.search.field {
            field
                .input
                .update(cx, |state, cx| state.set_value(text, window, cx));
        }
    }

    /// Moves the highlight in the list under the field by `delta`, wrapping.
    pub fn move_search_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let search = &mut self.pages.search;
        let n = search.entries().len() as isize;
        if n == 0 {
            return;
        }
        let at = match search.highlight {
            Some(i) => (i as isize + delta).rem_euclid(n),
            None if delta > 0 => 0,
            None => n - 1,
        };
        search.highlight = Some(at as usize);
        cx.notify();
    }

    /// Escape: closes the list under the field. `false` when it was closed.
    pub fn close_search_list(&mut self, cx: &mut Context<Self>) -> bool {
        let search = &mut self.pages.search;
        if search.dropdown().is_none() {
            return false;
        }
        search.closed = true;
        search.highlight = None;
        cx.notify();
        true
    }

    /// Drops one recent search and keeps the field (and its list) focused.
    pub fn forget_search(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.pages.search.recent.retain(|q| q != query);
        self.pages.search.highlight = None;
        self.save_searches();
        self.refocus_search(window, cx);
        cx.notify();
    }

    pub fn clear_searches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pages.search.recent.clear();
        self.pages.search.highlight = None;
        self.save_searches();
        self.refocus_search(window, cx);
        cx.notify();
    }

    fn refocus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(field) = &self.pages.search.field {
            field.input.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    fn save_searches(&mut self) {
        let list = self.pages.search.recent.clone();
        self.send(Command::SaveSearches(list));
    }

    pub(crate) fn on_suggestions(&mut self, input: String, items: Vec<String>) {
        let search = &mut self.pages.search;
        if input == search.text() {
            log::info!("{} suggestions for {input:?}", items.len());
            search.suggestions = items;
            search.highlight = None;
        }
    }

    pub(crate) fn on_searches(&mut self, saved: Vec<String>) {
        let recent = &mut self.pages.search.recent;
        // Searches made before the saved list arrived stay first.
        for query in saved {
            if !recent.iter().any(|q| q.eq_ignore_ascii_case(&query)) {
                recent.push(query);
            }
        }
        recent.truncate(encore_core::searches::KEEP);
        log::info!("{} recent searches", recent.len());
    }
}
