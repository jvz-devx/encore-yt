//! A key on the keyboard, as the shortcuts sheet and Play anything's hints
//! draw it: a small cap with a darker lower edge, so it reads as a key.

use gpui_kit::component::h_flex;
use gpui_kit::*;

use crate::theme::{Colors, Type, radius, space};

/// A cap's height (and its least width, so one letter makes a square).
const CAP: Pixels = px(24.);
/// The cap's lower edge.
const EDGE: Pixels = px(1.5);

pub fn keycap(label: &'static str, c: &Colors) -> Div {
    h_flex()
        .flex_none()
        .h(CAP)
        .min_w(CAP)
        .px(space::SM - space::XXS)
        .justify_center()
        .rounded(radius::SM)
        .bg(c.selected)
        .shadow(vec![BoxShadow {
            color: c.shadow.opacity(0.55),
            offset: point(px(0.), -EDGE),
            blur_radius: px(0.),
            spread_radius: px(0.),
            inset: true,
        }])
        .type_caption()
        .text_color(c.text)
        .child(label)
}

/// One key combination: its caps side by side ("Shift" "→").
pub fn combo(keys: &[&'static str], c: &Colors) -> Div {
    h_flex()
        .flex_none()
        .gap(space::XS)
        .children(keys.iter().map(|k| keycap(k, c)))
}
