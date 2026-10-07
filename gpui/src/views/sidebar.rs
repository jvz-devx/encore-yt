//! Navigation: the brand, Home, Explore and Library, then the library
//! (`library`): New playlist, Liked music and the account's playlists, and
//! Recently played; signed out, Explore's shortcuts (`signed_out`). Back
//! and Forward sit in the top bar. Below `size::RAIL_BELOW` it is a rail of
//! icons and covers with tooltips.

mod library;
mod signed_out;

use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Account, Target};

use super::widgets;
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::nav::{LibraryTab, View};
use crate::theme::{self, Colors, Type, radius, size, space};

pub fn sidebar(app: &MusicApp, window: &Window, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let c = theme::colors(cx);
    let rail = window.viewport_size().width < size::RAIL_BELOW;
    let library = matches!(app.pages.view, View::Library(_));
    // A Home mood (Energize, Relax…) is still Home.
    let home = match &app.pages.view {
        View::Home => true,
        View::Page(Target::Browse { id, .. }) => id == "FEmusic_home",
        _ => false,
    };
    // Library opens on the tab it was left at.
    let library_view = match app.pages.view {
        View::Library(tab) => View::Library(tab),
        _ => View::Library(LibraryTab::Playlists),
    };
    let explore = app.pages.view == View::Explore;
    let nav = |label: &'static str, icon, active, view: View, cx: &mut Context<MusicApp>| {
        item(label.into(), icon, active, rail, &c)
            .on_click(cx.listener(move |this, _, _, cx| this.open(view.clone(), cx)))
    };
    let below = match &app.account.account {
        Account::SignedIn { .. } => Some(library::library(app, rail, &c, cx).into_any_element()),
        Account::SignedOut { .. } | Account::Unverified { .. } => {
            Some(signed_out::signed_out(app, rail, &c, cx).into_any_element())
        }
        // Nothing is claimed before the account is known.
        Account::Checking => None,
    };
    v_flex()
        .w(if rail {
            size::SIDEBAR_RAIL
        } else {
            size::SIDEBAR
        })
        .flex_none()
        .h_full()
        .child(
            v_flex()
                .px(space::MD)
                .gap(space::XS)
                .child(brand(rail, &c))
                .child(nav("Home", IconName::House, home, View::Home, cx))
                .child(nav(
                    "Explore",
                    IconName::Compass,
                    explore,
                    View::Explore,
                    cx,
                ))
                .child(nav(
                    "Library",
                    IconName::LibraryBig,
                    library,
                    library_view,
                    cx,
                )),
        )
        .children(below.map(scroll))
}

/// The part under the navigation, scrolling on its own.
fn scroll(content: AnyElement) -> impl IntoElement {
    div().flex_1().min_h_0().w_full().mt(space::XL).child(
        v_flex()
            .size_full()
            .px(space::MD)
            .pb(space::LG)
            .child(content)
            .overflow_y_scrollbar(),
    )
}

/// The mark (a play glyph on a signal tile) and the app's name, the height
/// of the top bar so both share a baseline. The rail keeps the mark.
fn brand(rail: bool, c: &Colors) -> impl IntoElement {
    h_flex()
        .h(size::TOP_BAR)
        .flex_none()
        .mt(space::SM)
        .gap(space::MD)
        .map(|s| {
            if rail {
                s.justify_center()
            } else {
                s.px(space::MD)
            }
        })
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
        .when(!rail, |s| s.child(div().type_heading().child("Music")))
}

/// A navigation item: icon and label, or the icon alone on the rail (the
/// label as its tooltip). The caller adds the click.
fn item(
    label: SharedString,
    icon: IconName,
    active: bool,
    rail: bool,
    c: &Colors,
) -> Stateful<Div> {
    let fg = if active { c.text } else { c.text_muted };
    let (hover, pressed, text) = (c.hover, c.pressed, c.text);
    h_flex()
        .id(label.clone())
        .flex_none()
        .h(size::NAV_ITEM)
        .gap(space::MD)
        .rounded(radius::MD)
        .type_label()
        .text_color(fg)
        .cursor_pointer()
        .map(|s| {
            if rail {
                s.justify_center()
            } else {
                s.px(space::MD)
            }
        })
        .when(active, |s| s.bg(c.selected))
        .when(!active, |s| s.hover(move |s| s.bg(hover).text_color(text)))
        .active(move |s| s.bg(pressed))
        // No colour of its own: the icon follows the label on hover.
        .child(Icon::new(icon).size(size::ICON))
        .map(|s| {
            if rail {
                s.tooltip(widgets::tooltip(label))
            } else {
                s.child(div().truncate().child(label))
            }
        })
}
