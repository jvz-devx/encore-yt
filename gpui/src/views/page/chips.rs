//! Chips above a page (DESIGN.md "Chip"): Home's moods, search filters, an
//! artist's discography, and the Library's tabs. The selected chip is
//! filled like the primary button.

use gpui_kit::component::h_flex;
use gpui_kit::*;
use ytfast::model::Chip;

use super::Ctx;
use crate::app::MusicApp;
use crate::nav::LibraryTab;
use crate::theme::{Colors, Type, radius, size, space};

pub fn page_chips(chips: &[Chip], ctx: &Ctx, cx: &mut Context<MusicApp>) -> AnyElement {
    h_flex()
        .w_full()
        .flex_wrap()
        .gap(space::SM)
        .children(chips.iter().enumerate().map(|(i, chip)| {
            let key = ctx.key.clone();
            pill(
                ctx.id(format!("chip:{i}")),
                &chip.text,
                chip.selected,
                &ctx.c,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose_chip(&key, i, cx)))
        }))
        .into_any_element()
}

pub fn library_tabs(current: LibraryTab, ctx: &Ctx, cx: &mut Context<MusicApp>) -> AnyElement {
    h_flex()
        .w_full()
        .flex_wrap()
        .gap(space::SM)
        .children(LibraryTab::ALL.into_iter().map(|tab| {
            pill(
                SharedString::from(format!("library-tab:{}", tab.label())),
                tab.label(),
                tab == current,
                &ctx.c,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_library(tab, cx)))
        }))
        .into_any_element()
}

/// A 36 px pill: `raised` at rest, `primary` when selected.
fn pill(id: SharedString, label: &str, selected: bool, c: &Colors) -> Stateful<Div> {
    let (bg, hover, fg) = if selected {
        (c.primary, c.primary_hover, c.primary_foreground)
    } else {
        (c.raised, c.overlay, c.text)
    };
    h_flex()
        .id(id)
        .flex_none()
        .h(size::CHIP)
        .px(space::LG)
        .rounded(radius::FULL)
        .bg(bg)
        .text_color(fg)
        .type_label()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.9))
        .child(label.to_string())
}
