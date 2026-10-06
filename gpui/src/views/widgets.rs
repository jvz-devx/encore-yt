//! The design system's shared recipes (DESIGN.md "Components"): icons,
//! buttons, covers and loading placeholders. Views compose these instead of
//! restyling the same things.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::assets::Glyph;
use crate::theme::{Colors, Type, radius, size, space};

/// A Lucide icon at `px` size in `color`.
pub fn icon(name: IconName, px: Pixels, color: Hsla) -> Icon {
    Icon::new(name).size(px).text_color(color)
}

/// A filled glyph (play, pause, skip) at `px` size in `color`.
pub fn glyph(glyph: Glyph, px: Pixels, color: Hsla) -> Icon {
    Icon::default()
        .path(glyph.path())
        .size(px)
        .text_color(color)
}

/// A round, borderless icon button (36 px); colour its icon with
/// [`toggle_color`] when it is a toggle.
pub fn icon_button(
    id: impl Into<ElementId>,
    content: impl IntoElement,
    c: &Colors,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .size(size::ICON_BUTTON)
        .justify_center()
        .rounded(radius::FULL)
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(content)
}

/// The colour of a toggle's icon: signal when on, muted when off.
pub fn toggle_color(on: bool, c: &Colors) -> Hsla {
    if on { c.signal } else { c.text_muted }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pill {
    /// The one main action of a view: Play.
    Primary,
    /// Actions next to it: Shuffle, Radio, Try again.
    Secondary,
}

/// A pill button with an optional leading icon (36 px tall).
pub fn pill_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    leading: Option<Icon>,
    kind: Pill,
    c: &Colors,
) -> Stateful<Div> {
    let (bg, hover, fg) = match kind {
        Pill::Primary => (c.primary, c.primary_hover, c.primary_foreground),
        Pill::Secondary => (c.raised, c.overlay, c.text),
    };
    h_flex()
        .id(id)
        .flex_none()
        .h(size::CHIP)
        .pl(if leading.is_some() {
            space::MD
        } else {
            space::LG
        })
        .pr(space::LG)
        .gap(space::SM)
        .rounded(radius::FULL)
        .bg(bg)
        .text_color(fg)
        .type_label()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.opacity(0.9))
        .children(leading.map(|i| i.text_color(fg)))
        .child(label.into())
}

/// A cover image with its outline, rounded (or round). A placeholder (a
/// note on the raised fill) sits under the image, so a cover that is still
/// loading, failed or has no URL looks deliberate.
pub fn cover(url: Option<SharedString>, side: Pixels, round: bool, c: &Colors) -> Div {
    let corner = if round {
        radius::FULL
    } else if side >= size::HEADER_COVER {
        radius::LG
    } else if side >= size::CARD {
        radius::MD
    } else if side >= size::PLAYER_COVER {
        radius::SM
    } else {
        radius::XS
    };
    h_flex()
        .relative()
        .flex_none()
        .size(side)
        .justify_center()
        .rounded(corner)
        .bg(c.raised)
        .child(icon(
            IconName::Music,
            (side * 0.28).clamp(px(14.), px(48.)),
            c.text_faint,
        ))
        .children(url.map(|url| {
            img(url)
                .absolute()
                .top_0()
                .left_0()
                .size(side)
                .rounded(corner)
                .object_fit(ObjectFit::Cover)
        }))
        // The outline sits inside the edge so covers don't bleed into the
        // surface (white on dark, black on light).
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(corner)
                .border_1()
                .border_color(c.outline),
        )
}

/// A pulsing placeholder block for content that is loading.
pub fn skeleton(w: Pixels, h: Pixels, corner: Pixels, c: &Colors) -> impl IntoElement {
    div()
        .flex_none()
        .w(w)
        .h(h)
        .rounded(corner)
        .bg(c.raised)
        .with_animation(
            "skeleton",
            Animation::new(std::time::Duration::from_millis(1600))
                .repeat()
                .with_easing(pulsating_between(0.55, 1.)),
            |el, opacity| el.opacity(opacity),
        )
}

/// A muted line of text, one line, ellipsised.
pub fn muted_line(text: impl Into<SharedString>, c: &Colors) -> Div {
    div()
        .w_full()
        .truncate()
        .type_small()
        .text_color(c.text_muted)
        .child(text.into())
}
