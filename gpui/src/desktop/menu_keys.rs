//! M29: one keyboard model for every menu-like list (the context and ⋮
//! menus, the account menu, the sleep timer): ↑/↓ (and Tab) move one
//! highlight that the pointer moves too, wrapping; Home/End jump to the
//! ends; a letter jumps to the next entry starting with it; Enter or Space
//! chooses; → opens a submenu and ← closes it; Esc closes and gives the
//! keyboard back to where it was.
//!
//! The keys bind in the `MusicMenu` key context, where single-key
//! shortcuts stay out; each list answers the actions on its own element.

use gpui_kit::*;

actions!(
    music_menu,
    [
        MenuUp, MenuDown, MenuFirst, MenuLast, MenuChoose, MenuClose, MenuIn, MenuOut
    ]
);

/// The key context of an open menu.
pub const CONTEXT: &str = "MusicMenu";

pub fn bind_keys(cx: &mut App) {
    let menu = Some(CONTEXT);
    cx.bind_keys([
        KeyBinding::new("up", MenuUp, menu),
        KeyBinding::new("shift-tab", MenuUp, menu),
        KeyBinding::new("down", MenuDown, menu),
        KeyBinding::new("tab", MenuDown, menu),
        KeyBinding::new("home", MenuFirst, menu),
        KeyBinding::new("pageup", MenuFirst, menu),
        KeyBinding::new("end", MenuLast, menu),
        KeyBinding::new("pagedown", MenuLast, menu),
        KeyBinding::new("enter", MenuChoose, menu),
        KeyBinding::new("space", MenuChoose, menu),
        KeyBinding::new("right", MenuIn, menu),
        KeyBinding::new("left", MenuOut, menu),
        KeyBinding::new("escape", MenuClose, menu),
    ]);
}

/// The highlight `delta` places on from `selected` in a list of `count`,
/// wrapping; from nothing, ↓ starts at the top and ↑ at the bottom.
pub fn step(selected: Option<usize>, delta: isize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let n = count as isize;
    Some(match selected {
        Some(i) => (i as isize + delta).rem_euclid(n) as usize,
        None if delta > 0 => 0,
        None => count - 1,
    })
}

/// Type-ahead: the next entry after `selected` whose label starts with
/// `letter`, going round.
pub fn find_letter<'a>(
    labels: impl IntoIterator<Item = &'a str>,
    selected: Option<usize>,
    letter: char,
) -> Option<usize> {
    let labels: Vec<&str> = labels.into_iter().collect();
    let n = labels.len();
    let start = selected.map_or(0, |i| i + 1);
    (0..n).map(|k| (start + k) % n).find(|&i| {
        labels[i]
            .chars()
            .next()
            .is_some_and(|c| c.to_lowercase().eq(letter.to_lowercase()))
    })
}

/// The letter or digit a key press types, when it is a plain one (no
/// Ctrl, Alt or Cmd): what type-ahead looks for.
pub fn typed_letter(event: &KeyDownEvent) -> Option<char> {
    let m = &event.keystroke.modifiers;
    if m.control || m.alt || m.platform || m.function {
        return None;
    }
    let mut chars = event.keystroke.key.chars();
    let c = chars.next()?;
    (chars.next().is_none() && c.is_alphanumeric()).then_some(c)
}

/// A menu-like list's keyboard state: its focus, its highlight, and the
/// focus it was opened from.
pub struct ListNav {
    pub focus: FocusHandle,
    pub selected: Option<usize>,
    back: Option<FocusHandle>,
}

impl ListNav {
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle(),
            selected: None,
            back: None,
        }
    }

    /// The list opens: it takes the keyboard, nothing highlighted yet.
    pub fn open(&mut self, window: &mut Window, cx: &mut App) {
        let from = window.focused(cx).filter(|f| *f != self.focus);
        if from.is_some() {
            self.back = from;
        }
        self.selected = None;
        window.focus(&self.focus, cx);
    }

    /// The list closes: the keyboard goes back to where it was, if the
    /// list still had it.
    pub fn close(&mut self, root: &FocusHandle, window: &mut Window, cx: &mut App) {
        self.selected = None;
        let back = self.back.take();
        if self.focus.is_focused(window) {
            give_back(back, root, window, cx);
        }
    }

    pub fn step(&mut self, delta: isize, count: usize) {
        self.selected = step(self.selected, delta, count);
    }

    pub fn ends(&mut self, last: bool, count: usize) {
        self.selected = (count > 0).then(|| if last { count - 1 } else { 0 });
    }

    pub fn letter<'a>(&mut self, labels: impl IntoIterator<Item = &'a str>, letter: char) -> bool {
        match find_letter(labels, self.selected, letter) {
            Some(i) => {
                self.selected = Some(i);
                true
            }
            None => false,
        }
    }
}

/// Focuses `back` if it is still drawn in the window (under `root`, the
/// app's own focus), else `root`.
pub fn give_back(back: Option<FocusHandle>, root: &FocusHandle, window: &mut Window, cx: &mut App) {
    let to = back
        .filter(|b| b == root || root.contains(b, window))
        .unwrap_or_else(|| root.clone());
    window.focus(&to, cx);
}
