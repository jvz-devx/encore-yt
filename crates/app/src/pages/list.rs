//! A page's virtual list: GPUI's `ListState`, kept in step with the page's
//! entries (header, chips, shelves, and one entry per row of a song list) so
//! only what is on screen is laid out, and each page keeps its scroll
//! position for Back and Forward.

use gpui_kit::*;

/// How far past the visible area entries are laid out ahead of time; the
/// page asks for its continuation when its end comes this close.
const OVERDRAW: Pixels = px(900.);

pub struct PageList {
    pub state: ListState,
    /// One signature per entry as last laid out; a changed entry is
    /// measured again.
    sigs: Vec<u64>,
}

impl Default for PageList {
    fn default() -> Self {
        Self {
            state: ListState::new(0, ListAlignment::Top, OVERDRAW),
            sigs: Vec::new(),
        }
    }
}

impl PageList {
    /// Brings the list's entries in line with `sigs`: entries up to the
    /// first difference keep their measured heights, the rest are measured
    /// again. Appending (a continuation) leaves the scroll position alone.
    pub fn sync(&mut self, sigs: Vec<u64>) {
        if sigs == self.sigs {
            return;
        }
        let same = self
            .sigs
            .iter()
            .zip(&sigs)
            .take_while(|(a, b)| a == b)
            .count();
        self.state.splice(same..self.sigs.len(), sigs.len() - same);
        self.sigs = sigs;
    }

    pub fn scroll_to_top(&self) {
        self.state.scroll_to(ListOffset::default());
    }
}
