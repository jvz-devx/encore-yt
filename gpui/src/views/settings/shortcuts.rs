//! Settings → Keyboard shortcuts: every shortcut, grouped as the `?` sheet
//! groups them and drawn by the sheet's own lines, from the one table
//! (`desktop::SHORTCUTS`). Two columns when there is room.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::overlays::shortcuts::{COLUMNS, lines};
use crate::desktop::Group;
use crate::theme::{Colors, space};

pub fn page(two_columns: bool, c: &Colors) -> Vec<AnyElement> {
    if !two_columns {
        return COLUMNS
            .iter()
            .flat_map(|groups| groups.iter())
            .map(|g| group(*g, c))
            .collect();
    }
    vec![
        h_flex()
            .items_start()
            .gap(space::XL)
            .children(COLUMNS.iter().map(|groups| {
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(space::XL)
                    .children(groups.iter().map(|g| group(*g, c)))
            }))
            .into_any_element(),
    ]
}

fn group(group: Group, c: &Colors) -> AnyElement {
    super::section(
        group.title(),
        c,
        lines(group, c).map(|line| line.into_any_element()),
    )
}
