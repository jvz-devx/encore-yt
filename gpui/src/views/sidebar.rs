//! Navigation: Home, Explore, Library, and Back/Forward.

use gpui_kit::assets::IconName as Icon;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::sidebar::{
    Sidebar, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::{Disableable, StyledExt, h_flex};
use gpui_kit::*;

use crate::app::MusicApp;
use crate::nav::{LibraryTab, View};

pub fn sidebar(app: &MusicApp, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let library = matches!(app.view, View::Library(_));
    Sidebar::new("nav")
        .w(px(220.))
        .header(
            SidebarHeader::new().child(
                h_flex()
                    .w_full()
                    .gap_1()
                    .child(div().flex_1().font_bold().child("Music"))
                    .child(
                        Button::new("back")
                            .ghost()
                            .icon(Icon::ChevronLeft)
                            .disabled(app.history.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.back(cx))),
                    )
                    .child(
                        Button::new("forward")
                            .ghost()
                            .icon(Icon::ChevronRight)
                            .disabled(app.forward.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.go_forward(cx))),
                    ),
            ),
        )
        .child(
            SidebarGroup::new("Browse").child(SidebarMenu::new().children([
                item("Home", Icon::House, app.view == View::Home, View::Home, cx),
                item(
                    "Explore",
                    Icon::Compass,
                    app.view == View::Explore,
                    View::Explore,
                    cx,
                ),
                item(
                    "Library",
                    Icon::LibraryBig,
                    library,
                    View::Library(LibraryTab::Playlists),
                    cx,
                ),
            ])),
        )
}

fn item(
    label: &'static str,
    icon: Icon,
    active: bool,
    view: View,
    cx: &mut Context<MusicApp>,
) -> SidebarMenuItem {
    SidebarMenuItem::new(label)
        .icon(icon)
        .active(active)
        .on_click(cx.listener(move |this, _, _, cx| this.open(view.clone(), cx)))
}
