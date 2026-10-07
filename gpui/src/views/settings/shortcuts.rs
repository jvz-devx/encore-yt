//! Settings → Keyboard shortcuts: a tab per group of the `?` sheet, its
//! shortcuts drawn by the sheet's own lines from the one table
//! (`desktop::SHORTCUTS`).

use gpui_kit::*;

use super::super::overlays::shortcuts::lines;
use crate::desktop::Group;
use crate::theme::Colors;

pub fn page(tab: usize, c: &Colors) -> Vec<AnyElement> {
    let group = Group::ALL.get(tab).copied().unwrap_or(Group::Playback);
    vec![super::section(
        "",
        c,
        lines(group, c).map(|line| line.into_any_element()),
    )]
}
