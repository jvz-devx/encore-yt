//! The account's YouTube channels (its own and any brand accounts) as rows
//! with photo, name and handle, a check on the one requests act as. The
//! account menu and Settings → Account list them; choosing one switches.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::Channel;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, size, space};

const PHOTO: Pixels = px(32.);

/// One row per channel, or none when the account has only one. In a menu
/// (`menu`) the menu draws the highlight; elsewhere the pointer does.
pub fn rows(
    app: &MusicApp,
    id: &'static str,
    menu: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Vec<Stateful<Div>> {
    let channels = &app.account.channels;
    if channels.len() < 2 {
        return Vec::new();
    }
    channels
        .iter()
        .enumerate()
        .map(|(i, channel)| row((id, i), channel, menu, c, cx))
        .collect()
}

fn row(
    id: impl Into<ElementId>,
    channel: &Channel,
    menu: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Stateful<Div> {
    let page_id = channel.page_id.clone();
    // The check keeps its place on every row, so the names line up.
    let mark = div().size(size::ICON).flex_none().children(
        channel
            .current
            .then(|| widgets::icon(IconName::Check, size::ICON, c.signal)),
    );
    h_flex()
        .id(id)
        .h(size::LIBRARY_ROW)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .when(!menu, |r| r.hover(|s| s.bg(c.hover)))
        .active(|s| s.bg(c.pressed))
        .child(super::avatar(
            channel.photo.as_deref(),
            &channel.name,
            PHOTO,
            cx,
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().type_label().truncate().child(channel.name.clone()))
                .children(channel.handle.clone().map(|handle| {
                    div()
                        .type_small()
                        .text_color(c.text_muted)
                        .truncate()
                        .child(handle)
                })),
        )
        .child(mark)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.close_account_menu(window, cx);
            this.use_channel(page_id.clone(), cx)
        }))
}
