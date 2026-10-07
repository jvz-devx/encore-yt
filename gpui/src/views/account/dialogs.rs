//! The account's dialogs over the window: New playlist, Edit playlist,
//! Delete playlist, and Save to playlist (the account's playlists, with a
//! filter). Enter confirms, Escape or a click outside cancels.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::account::Dialog;

use super::super::widgets::{self, Pill};
use crate::account::PlaylistChoice;
use crate::app::MusicApp;
use crate::theme::{self, Colors, Type, radius, size, space};

const WIDTH: Pixels = px(440.);
/// The playlist list scrolls past this height.
const LIST_HEIGHT: Pixels = px(320.);

pub fn dialog(
    app: &MusicApp,
    _window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    let dialog = app.account.state.dialog.as_ref()?;
    let c = theme::colors(cx);
    let body = match dialog {
        Dialog::NewPlaylist { .. } => playlist_form(app, "New playlist", "Create", true, &c, cx),
        Dialog::EditPlaylist { .. } => playlist_form(app, "Edit playlist", "Save", false, &c, cx),
        Dialog::DeletePlaylist { title, .. } => delete(title, &c, cx),
        Dialog::AddToPlaylist {
            tracks, selected, ..
        } => picker(app, tracks, *selected, &c, cx).into_any_element(),
    };
    Some(layer(app, body, &c, cx))
}

/// The scrim and the panel, with the dialog's keys.
fn layer(app: &MusicApp, body: AnyElement, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let panel = widgets::floating(c)
        .id("account-dialog")
        .track_focus(&app.account.focus)
        .w(WIDTH)
        .p(space::XL)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            match event.keystroke.key.as_str() {
                "escape" => this.close_account_dialog(window, cx),
                "enter" => this.confirm_account_dialog(window, cx),
                "down" => this.move_playlist_selection(true, cx),
                "up" => this.move_playlist_selection(false, cx),
                _ => return,
            }
            cx.stop_propagation();
        }))
        .child(body);
    widgets::scrim("account-dialog-scrim", c)
        .on_click(cx.listener(|this, _, window, cx| this.close_account_dialog(window, cx)))
        .child(widgets::settle_in("account-dialog-in", panel))
        .into_any_element()
}

fn title(text: impl Into<SharedString>) -> Div {
    div().type_heading().child(text.into())
}

/// New or Edit playlist: a name and a description.
fn playlist_form(
    app: &MusicApp,
    heading: &'static str,
    confirm: &'static str,
    new: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let Some(fields) = &app.account.fields else {
        return div().into_any_element();
    };
    let named = !fields.first.read(cx).value().trim().is_empty();
    v_flex()
        .gap(space::LG)
        .child(title(heading))
        .child(field("Name", &fields.first, c))
        .children(
            fields
                .second
                .as_ref()
                .map(|second| field("Description", second, c)),
        )
        .when(new, |col| {
            col.child(
                h_flex()
                    .gap(space::SM)
                    .type_small()
                    .text_color(c.text_muted)
                    .child(widgets::icon(IconName::Lock, size::ICON_SM, c.text_muted))
                    .child("Private: only you can see it"),
            )
        })
        .child(buttons(confirm, Pill::Primary, named, c, cx))
        .into_any_element()
}

/// A labelled text field: the label above a 40 px field on `raised`.
fn field(label: &'static str, state: &Entity<InputState>, c: &Colors) -> impl IntoElement {
    v_flex()
        .gap(space::SM)
        .child(div().type_label().text_color(c.text_muted).child(label))
        .child(
            Input::new(state)
                .h(px(40.))
                .px(space::MD)
                .rounded(radius::MD)
                .bg(c.raised)
                .border_color(c.raised)
                .type_body(),
        )
}

fn delete(playlist: &str, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    v_flex()
        .gap(space::LG)
        .child(title("Delete playlist"))
        .child(div().type_body().text_color(c.text_muted).child(format!(
            "“{playlist}” will be deleted from your account. This can't be undone."
        )))
        .child(buttons("Delete", Pill::Danger, true, c, cx))
        .into_any_element()
}

/// Cancel and the confirming button, at the right.
fn buttons(
    confirm: &'static str,
    kind: Pill,
    enabled: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    h_flex()
        .mt(space::SM)
        .gap(space::SM)
        .justify_end()
        .child(
            widgets::pill_button("dialog-cancel", "Cancel", None, Pill::Secondary, c)
                .on_click(cx.listener(|this, _, window, cx| this.close_account_dialog(window, cx))),
        )
        .child(
            widgets::pill_button("dialog-confirm", confirm, None, kind, c)
                .when(!enabled, |b| b.opacity(0.4).cursor_default())
                .when(enabled, |b| {
                    b.on_click(
                        cx.listener(|this, _, window, cx| this.confirm_account_dialog(window, cx)),
                    )
                }),
        )
}

/// Save to playlist: the filter, the account's playlists, New playlist.
fn picker(
    app: &MusicApp,
    tracks: &[ytfast::model::Track],
    selected: usize,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let what = match tracks {
        [one] => one.title.clone(),
        many => format!("{} songs", many.len()),
    };
    let list = match app.playlist_matches() {
        None => loading_rows(c).into_any_element(),
        Some(choices) if choices.is_empty() => {
            let empty = if app.account.state.dialog.as_ref().is_some_and(
                |d| matches!(d, Dialog::AddToPlaylist { filter, .. } if !filter.trim().is_empty()),
            ) {
                "No playlist has that name."
            } else {
                "You have no playlists yet."
            };
            div()
                .py(space::LG)
                .px(space::SM)
                .type_body()
                .text_color(c.text_muted)
                .child(empty)
                .into_any_element()
        }
        Some(choices) => playlist_list(choices, selected, c, cx).into_any_element(),
    };
    let new = tracks.to_vec();
    v_flex()
        .gap(space::LG)
        .child(
            v_flex()
                .gap(space::XXS)
                .child(title("Save to playlist"))
                .child(widgets::muted_line(what, c)),
        )
        .children(app.account.fields.as_ref().map(|f| {
            Input::new(&f.first)
                .prefix(widgets::icon(IconName::Search, size::ICON_SM, c.text_muted))
                .h(px(40.))
                .px(space::MD)
                .rounded(radius::FULL)
                .bg(c.raised)
                .border_color(c.raised)
                .type_body()
        }))
        .child(list)
        .child(
            h_flex()
                .justify_between()
                .child(
                    widgets::pill_button(
                        "picker-new",
                        "New playlist",
                        Some(widgets::icon(IconName::Plus, size::ICON_SM, c.text)),
                        Pill::Secondary,
                        c,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let dialog = Dialog::NewPlaylist {
                            title: String::new(),
                            description: String::new(),
                            tracks: new.clone(),
                        };
                        this.open_account_dialog(dialog, window, cx);
                    })),
                )
                .child(
                    widgets::pill_button("picker-cancel", "Cancel", None, Pill::Secondary, c)
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.close_account_dialog(window, cx)
                            }),
                        ),
                ),
        )
}

fn playlist_list(
    choices: Vec<PlaylistChoice>,
    selected: usize,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    v_flex()
        .id("picker-list")
        .max_h(LIST_HEIGHT)
        .overflow_y_scroll()
        .mx(-space::SM)
        .children(choices.into_iter().enumerate().map(|(i, choice)| {
            let id = choice.id.clone();
            h_flex()
                .id(SharedString::from(format!("picker:{id}")))
                .h(size::ROW)
                .px(space::SM)
                .gap(space::MD)
                .rounded(radius::MD)
                .cursor_pointer()
                .when(i == selected, |r| r.bg(c.selected))
                .hover(|r| r.bg(c.hover))
                .active(|r| r.bg(c.pressed))
                .child(widgets::cover(
                    choice.thumbnail.map(Into::into),
                    size::ROW_THUMB,
                    false,
                    c,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .type_label()
                        .child(choice.title),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    if let Some(Dialog::AddToPlaylist { selected, .. }) =
                        &mut this.account.state.dialog
                    {
                        *selected = i;
                    }
                    this.confirm_account_dialog(window, cx);
                }))
        }))
}

/// Library's playlists still loading: three rows' shapes, pulsing.
fn loading_rows(c: &Colors) -> impl IntoElement {
    v_flex().children((0..3).map(|_| {
        h_flex()
            .h(size::ROW)
            .gap(space::MD)
            .child(widgets::skeleton(
                size::ROW_THUMB,
                size::ROW_THUMB,
                radius::XS,
                c,
            ))
            .child(widgets::skeleton(px(180.), px(14.), radius::XS, c))
    }))
}
