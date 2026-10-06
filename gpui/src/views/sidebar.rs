//! Navigation: the brand, then Home, Explore and Library. Back/Forward sit
//! in the top bar.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets;
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::nav::{LibraryTab, View};
use crate::theme::{self, Colors, Type, radius, size, space};

pub fn sidebar(app: &MusicApp, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    let library = matches!(app.pages.view, View::Library(_));
    v_flex()
        .w(size::SIDEBAR)
        .flex_none()
        .h_full()
        .px(space::MD)
        .gap(space::XS)
        .child(brand(&c))
        .child(item(
            "Home",
            IconName::House,
            app.pages.view == View::Home,
            View::Home,
            &c,
            cx,
        ))
        .child(item(
            "Explore",
            IconName::Compass,
            app.pages.view == View::Explore,
            View::Explore,
            &c,
            cx,
        ))
        .child(item(
            "Library",
            IconName::LibraryBig,
            library,
            View::Library(LibraryTab::Playlists),
            &c,
            cx,
        ))
}

/// The mark (a play glyph on a signal tile) and the app's name, the height
/// of the top bar so both share a baseline.
fn brand(c: &Colors) -> impl IntoElement {
    h_flex()
        .h(size::TOP_BAR)
        .flex_none()
        .px(space::MD)
        .mt(space::SM)
        .gap(space::MD)
        .child(
            h_flex()
                .size(px(28.))
                .justify_center()
                .rounded(radius::MD)
                .bg(c.signal)
                // Nudged right: a triangle's visual centre is left of its box's.
                .pl(px(2.))
                .child(widgets::glyph(Glyph::Play, px(14.), c.on_media)),
        )
        .child(div().type_heading().child("Music"))
}

fn item(
    label: &'static str,
    icon: IconName,
    active: bool,
    view: View,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let fg = if active { c.text } else { c.text_muted };
    let (hover, pressed) = (c.hover, c.pressed);
    h_flex()
        .id(label)
        .h(size::NAV_ITEM)
        .px(space::MD)
        .gap(space::MD)
        .rounded(radius::MD)
        .type_label()
        .text_color(fg)
        .cursor_pointer()
        .when(active, |s| s.bg(c.selected))
        .when(!active, |s| {
            s.hover(move |s| s.bg(hover).text_color(c.text))
        })
        .active(move |s| s.bg(pressed))
        // No colour of its own: the icon follows the label on hover.
        .child(Icon::new(icon).size(size::ICON))
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| this.open(view.clone(), cx)))
}
