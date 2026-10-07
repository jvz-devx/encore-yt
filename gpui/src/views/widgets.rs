//! The design system's shared recipes (DESIGN.md "Components"): icons,
//! buttons, covers and loading placeholders. Views compose these instead of
//! restyling the same things.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::h_flex;
use gpui_kit::component::switch::Switch;
use gpui_kit::*;

use crate::assets::Glyph;
use crate::theme::{Colors, Type, elevation, motion, radius, size, space};

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
    /// Confirms something that can't be undone: Delete.
    Danger,
    /// A secondary action on a card that is itself tinted (Settings): a
    /// translucent fill, so it stands out from the card as `raised` does
    /// from the page.
    Tonal,
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
        Pill::Danger => (c.danger, c.danger.opacity(0.88), c.primary_foreground),
        Pill::Tonal => (c.hover, c.pressed, c.text),
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

/// A secondary pill that is a toggle (Save to library, Subscribe): its
/// icon is `signal` while on.
pub fn toggle_pill(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    leading: IconName,
    on: bool,
    c: &Colors,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .h(size::CHIP)
        .pl(space::MD)
        .pr(space::LG)
        .gap(space::SM)
        .rounded(radius::FULL)
        .bg(c.raised)
        .text_color(c.text)
        .type_label()
        .cursor_pointer()
        .hover(|s| s.bg(c.overlay))
        .active(|s| s.opacity(0.9))
        .child(icon(
            leading,
            size::ICON_SM,
            if on { c.signal } else { c.text },
        ))
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
            // Asks for the size drawn, not the large copy the parser names.
            img(SharedString::from(super::page::covers::sized(&url, side)))
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

/// A pulsing placeholder block for content that is loading; it holds
/// still when Settings → Motion turns the pulse off.
pub fn skeleton(w: Pixels, h: Pixels, corner: Pixels, c: &Colors) -> impl IntoElement {
    const PULSE: std::time::Duration = std::time::Duration::from_millis(1600);
    let block = div().flex_none().w(w).h(h).rounded(corner).bg(c.raised);
    match motion::scaled(motion::Kind::Skeleton, PULSE) {
        Some(period) => block
            .with_animation(
                "skeleton",
                Animation::new(period)
                    .repeat()
                    .with_easing(pulsating_between(0.55, 1.)),
                |el, opacity| el.opacity(opacity),
            )
            .into_any_element(),
        None => block.opacity(0.8).into_any_element(),
    }
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

/// An on/off switch; on is `signal` (DESIGN.md: a toggle that is on is live).
pub fn switch(id: impl Into<ElementId>, on: bool, c: &Colors) -> Switch {
    Switch::new(id).checked(on).color(c.signal)
}

/// A row in a menu or popover: 40 tall, an 18 px icon and a label. The
/// menu's padding is `XS`, so its `LG` corners stay concentric with these.
pub fn menu_item(
    id: impl Into<ElementId>,
    icon_name: IconName,
    label: impl Into<SharedString>,
    c: &Colors,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(size::NAV_ITEM)
        .px(space::MD)
        .gap(space::MD)
        .rounded(radius::MD)
        .type_label()
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(icon(icon_name, size::ICON, c.text_muted))
        .child(label.into())
}

/// A floating panel (menus, popovers, dialogs): `overlay` fill, `LG`
/// corners, high elevation.
pub fn floating(c: &Colors) -> Div {
    div()
        .bg(c.overlay)
        .rounded(radius::LG)
        .shadow(elevation::high(c))
}

/// The layer behind a dialog: a `scrim` over the window that centres its
/// child. Clicks on it close the dialog (`on_click`); the dialog's panel
/// stops them.
pub fn scrim(id: impl Into<ElementId>, c: &Colors) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(c.scrim)
        .occlude()
}

/// A panel's entrance: it fades in and settles up from 8 px below.
pub fn settle_in<E: Styled + IntoElement + 'static>(
    id: impl Into<ElementId>,
    kind: motion::Kind,
    el: E,
) -> AnyElement {
    motion::animate(el, id, kind, motion::BASE, |el, t| {
        el.opacity(t).mt(px(8.) * (1. - t))
    })
}

/// A tooltip builder for `.tooltip(..)`: a short label in the kit's
/// popover style.
pub fn tooltip(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx)
}

/// An empty or failed state: a 48 px raised disc with a muted icon, a
/// heading saying what happened and a muted detail line.
pub fn empty_state(
    icon_name: IconName,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    c: &Colors,
) -> Div {
    gpui_kit::component::v_flex()
        .items_center()
        .gap(space::MD)
        .px(space::XL)
        .child(
            h_flex()
                .size(px(48.))
                .justify_center()
                .rounded(radius::FULL)
                .bg(c.raised)
                .child(icon(icon_name, px(22.), c.text_muted)),
        )
        .child(div().text_center().type_heading().child(title.into()))
        .child(
            div()
                .max_w(px(320.))
                .text_center()
                .type_small()
                .text_color(c.text_muted)
                .child(detail.into()),
        )
}
