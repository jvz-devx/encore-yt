//! The sign-in sheet (M18): three ways to give Music a YouTube Music
//! session, then each one's progress until the account is checked.
//! `crate::sign_in` holds the state and does the work.

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::sign_in::{MUSIC_URL, Route, Step};
use crate::theme::{self, Colors, Type, radius, size, space};

const WIDTH: Pixels = px(480.);
/// A route's row: room for a title and a two-line description.
const ROUTE_ROW: Pixels = px(64.);
/// The number disc in front of each step of Paste cookies.
const STEP_DISC: Pixels = px(22.);

pub fn sheet(
    app: &MusicApp,
    _window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    let sign_in = &app.sign_in;
    if !sign_in.open {
        return None;
    }
    let c = theme::colors(cx);
    let body = match sign_in.route {
        Route::Choose => choose(&c, cx),
        Route::Browser => browser(&sign_in.step, &c, cx),
        Route::File => file(app, &c, cx),
        Route::Paste => paste(app, &c, cx),
    };
    let panel = widgets::floating(&c)
        .id("sign-in-sheet")
        .key_context(crate::account::DIALOG_CONTEXT)
        .track_focus(&app.account.focus)
        .w(WIDTH)
        .p(space::XL)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            v_flex()
                .gap(space::LG)
                .child(header(sign_in.route, &c, cx))
                .child(body),
        );
    Some(
        widgets::scrim("sign-in-scrim", &c)
            .on_click(cx.listener(|this, _, window, cx| this.close_sign_in(window, cx)))
            .child(widgets::settle_in(
                "sign-in-in",
                theme::motion::Kind::Panels,
                panel,
            ))
            .into_any_element(),
    )
}

/// The title, with Back on a route's page and Close at the right.
fn header(route: Route, c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    let title = match route {
        Route::Choose => "Sign in to YouTube Music",
        Route::Browser => "Sign in with your browser",
        Route::File => "Import a cookies file",
        Route::Paste => "Paste cookies",
    };
    h_flex()
        .gap(space::SM)
        .when(route != Route::Choose, |row| {
            row.ml(-space::SM).child(
                widgets::icon_button(
                    "sign-in-back",
                    widgets::icon(IconName::ChevronLeft, size::ICON, c.text_muted),
                    c,
                )
                .tooltip(widgets::tooltip("Back"))
                .on_click(cx.listener(|this, _, window, cx| this.sign_in_back(window, cx))),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .type_heading()
                .child(title),
        )
        .child(
            widgets::icon_button(
                "sign-in-close",
                widgets::icon(IconName::X, size::ICON, c.text_muted),
                c,
            )
            .mr(-space::SM)
            .tooltip(widgets::tooltip("Close"))
            .on_click(cx.listener(|this, _, window, cx| this.close_sign_in(window, cx))),
        )
}

/// The three routes, the browser first.
fn choose(c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    v_flex()
        .gap(space::LG)
        .child(paragraph(
            "Encore uses the YouTube Music sign-in from a browser on this computer, or cookies you \
             give it.",
            c,
        ))
        .child(
            v_flex()
                .mx(-space::MD)
                .gap(space::XS)
                .child(route_row(
                    Route::Browser,
                    IconName::Globe,
                    "Sign in with your browser",
                    "Opens YouTube Music. Sign in there, and Music connects by itself.",
                    true,
                    c,
                    cx,
                ))
                .child(route_row(
                    Route::File,
                    IconName::FileUp,
                    "Import a cookies file",
                    "A cookies.txt exported from a browser where you're signed in.",
                    false,
                    c,
                    cx,
                ))
                .child(route_row(
                    Route::Paste,
                    IconName::ClipboardPaste,
                    "Paste cookies",
                    "The Cookie header, copied from your browser's developer tools.",
                    false,
                    c,
                    cx,
                )),
        )
        .child(
            h_flex()
                .gap(space::SM)
                .type_small()
                .text_color(c.text_faint)
                .child(widgets::icon(IconName::Lock, size::ICON_SM, c.text_faint))
                .child("Cookies stay on this computer, in a file only you can read."),
        )
        .into_any_element()
}

/// A route: a disc with its icon (filled for the suggested one), a title
/// and what it does.
#[allow(clippy::too_many_arguments)]
fn route_row(
    route: Route,
    icon: IconName,
    title: &'static str,
    detail: &'static str,
    suggested: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let (disc, glyph) = if suggested {
        (c.primary, c.primary_foreground)
    } else {
        (c.raised, c.text)
    };
    h_flex()
        .id(title)
        .min_h(ROUTE_ROW)
        .px(space::MD)
        .py(space::SM)
        .gap(space::MD)
        .rounded(radius::MD)
        .cursor_pointer()
        .hover(|s| s.bg(c.hover))
        .active(|s| s.bg(c.pressed))
        .child(
            h_flex()
                .size(px(40.))
                .flex_none()
                .justify_center()
                .rounded(radius::FULL)
                .bg(disc)
                .child(widgets::icon(icon, size::ICON, glyph)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_label().child(title))
                .child(div().type_small().text_color(c.text_muted).child(detail)),
        )
        .child(widgets::icon(
            IconName::ChevronRight,
            size::ICON_SM,
            c.text_faint,
        ))
        .on_click(cx.listener(move |this, _, window, cx| this.sign_in_route(route, window, cx)))
}

/// The browser route: waiting, given up, found, or failed.
fn browser(step: &Step, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let col = v_flex().gap(space::LG);
    match step {
        Step::Waiting { checked, .. } => col
            .child(working("Waiting for you to sign in", c))
            .child(paragraph(
                "YouTube Music is open in your browser. Sign in there, and Music connects as soon \
                 as it finds the sign-in.",
                c,
            ))
            .child(checked_line(checked.as_deref(), c))
            .child(
                h_flex()
                    .gap(space::SM)
                    .justify_end()
                    .child(
                        widgets::pill_button(
                            "sign-in-reopen",
                            "Open YouTube Music again",
                            None,
                            Pill::Secondary,
                            c,
                        )
                        .on_click(cx.listener(|_, _, _, cx| cx.open_url(MUSIC_URL))),
                    )
                    .child(cancel(c, cx)),
            ),
        Step::TimedOut { checked } => col
            .child(alert("No browser is signed in to YouTube Music yet.", c))
            .child(checked_line(Some(checked), c))
            .child(paragraph(
                "Sign in at music.youtube.com and try again, or import or paste cookies instead.",
                c,
            ))
            .child(try_again(c, cx)),
        Step::Failed(reason) => col.child(alert(reason, c)).child(try_again(c, cx)),
        Step::Connecting { .. } | Step::Saving | Step::Idle => col
            .children(progress(step, c))
            .child(h_flex().justify_end().child(cancel(c, cx))),
    }
    .into_any_element()
}

/// Which browser profiles the last look went through.
fn checked_line(checked: Option<&[String]>, c: &Colors) -> impl IntoElement {
    let text = match checked {
        None => "Looking for browsers on this computer…".to_string(),
        Some([]) => format!(
            "No browser profile found. Encore looks in {}.",
            list(&encore_core::auth::supported_browsers())
        ),
        Some(found) => format!("Checking {}.", list(found)),
    };
    div().type_small().text_color(c.text_muted).child(text)
}

/// "A, B and C".
fn list(items: &[impl AsRef<str>]) -> String {
    match items {
        [] => String::new(),
        [one] => one.as_ref().to_string(),
        [rest @ .., last] => format!(
            "{} and {}",
            rest.iter()
                .map(AsRef::as_ref)
                .collect::<Vec<_>>()
                .join(", "),
            last.as_ref()
        ),
    }
}

/// Import a cookies file: the file picker, or a typed path.
fn file(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let busy = app.sign_in.busy();
    v_flex()
        .gap(space::LG)
        .child(paragraph(
            "Export cookies.txt (Netscape format) from a browser where you're signed in to YouTube \
             Music, with yt-dlp or a cookies.txt extension. Encore keeps only the YouTube and \
             Google cookies.",
            c,
        ))
        .child(
            h_flex().child(
                widgets::pill_button(
                    "sign-in-choose-file",
                    "Choose file…",
                    Some(widgets::icon(IconName::FileUp, size::ICON_SM, c.text)),
                    Pill::Primary,
                    c,
                )
                .when(!busy, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| this.pick_cookie_file(cx)))
                }),
            ),
        )
        .children(app.sign_in.path.as_ref().map(|path| {
            v_flex()
                .gap(space::SM)
                .child(
                    div()
                        .type_label()
                        .text_color(c.text_muted)
                        .child("Or type its path"),
                )
                .child(
                    h_flex()
                        .gap(space::SM)
                        .child(div().flex_1().min_w_0().child(field(path, c)))
                        .child(
                            widgets::pill_button(
                                "sign-in-import",
                                "Import",
                                None,
                                Pill::Secondary,
                                c,
                            )
                            .when(!busy, |b| {
                                b.on_click(cx.listener(|this, _, _, cx| this.import_typed_path(cx)))
                            }),
                        ),
                )
        }))
        .children(progress(&app.sign_in.step, c))
        .into_any_element()
}

/// Paste cookies: where to find the header, the field, Sign in.
fn paste(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let filled = app
        .sign_in
        .paste
        .as_ref()
        .is_some_and(|p| !p.read(cx).value().trim().is_empty());
    let enabled = filled && !app.sign_in.busy();
    v_flex()
        .gap(space::LG)
        .child(
            v_flex()
                .gap(space::MD)
                .child(step_line(
                    1,
                    "Open music.youtube.com in a browser where you're signed in.",
                    c,
                ))
                .child(step_line(
                    2,
                    "Open the developer tools (F12), choose Network, and reload the page.",
                    c,
                ))
                .child(step_line(
                    3,
                    "Select a request to music.youtube.com and copy the value of its Cookie \
                     request header.",
                    c,
                )),
        )
        .children(app.sign_in.paste.as_ref().map(|p| field(p, c)))
        .children(progress(&app.sign_in.step, c))
        .child(
            h_flex().justify_end().child(
                widgets::pill_button(
                    "sign-in-paste",
                    "Sign in",
                    Some(widgets::icon(IconName::LogIn, size::ICON_SM, c.text)),
                    Pill::Primary,
                    c,
                )
                .when(!enabled, |b| b.opacity(0.4).cursor_default())
                .when(enabled, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| this.paste_cookies(cx)))
                }),
            ),
        )
        .into_any_element()
}

/// One numbered step of the paste instructions.
fn step_line(n: usize, text: &'static str, c: &Colors) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap(space::MD)
        .child(
            h_flex()
                .size(STEP_DISC)
                .flex_none()
                .justify_center()
                .rounded(radius::FULL)
                .bg(c.raised)
                .type_caption()
                .tabular()
                .text_color(c.text_muted)
                .child(n.to_string()),
        )
        .child(div().flex_1().min_w_0().type_body().child(text))
}

/// A 40 px field on `raised`, as in the playlist dialogs.
fn field(state: &Entity<InputState>, c: &Colors) -> impl IntoElement {
    Input::new(state)
        .h(px(40.))
        .px(space::MD)
        .rounded(radius::MD)
        .bg(c.raised)
        .border_color(c.raised)
        .type_body()
}

/// Saving, connecting or failed, under a route's controls.
fn progress(step: &Step, c: &Colors) -> Option<AnyElement> {
    match step {
        Step::Saving => Some(working("Saving the cookies…", c).into_any_element()),
        Step::Connecting { source } => Some(
            v_flex()
                .gap(space::XS)
                .child(working("Checking your account…", c))
                .child(
                    div()
                        .pl(px(26.))
                        .child(widgets::muted_line(format!("Found {source}"), c)),
                )
                .into_any_element(),
        ),
        Step::Failed(reason) => Some(alert(reason, c).into_any_element()),
        _ => None,
    }
}

/// A spinner and what is happening.
fn working(text: impl Into<SharedString>, c: &Colors) -> impl IntoElement {
    h_flex()
        .gap(space::SM + space::XXS)
        .type_label()
        .child(Spinner::new().color(c.text_muted))
        .child(text.into())
}

/// What went wrong, as the error strip shows it.
fn alert(text: &str, c: &Colors) -> impl IntoElement {
    h_flex()
        .items_start()
        .gap(space::SM)
        .p(space::MD)
        .rounded(radius::MD)
        .bg(c.danger_soft)
        .type_small()
        .child(widgets::icon(
            IconName::CircleAlert,
            size::ICON_SM,
            c.danger,
        ))
        .child(div().flex_1().min_w_0().child(text.to_string()))
}

fn paragraph(text: &'static str, c: &Colors) -> impl IntoElement {
    div().type_body().text_color(c.text_muted).child(text)
}

fn cancel(c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    widgets::pill_button("sign-in-cancel", "Cancel", None, Pill::Secondary, c)
        .on_click(cx.listener(|this, _, window, cx| this.sign_in_back(window, cx)))
}

fn try_again(c: &Colors, cx: &mut Context<MusicApp>) -> impl IntoElement {
    h_flex()
        .gap(space::SM)
        .justify_end()
        .child(cancel(c, cx))
        .child(
            widgets::pill_button("sign-in-retry", "Try again", None, Pill::Primary, c)
                .on_click(cx.listener(|this, _, _, cx| this.sign_in_with_browser(cx))),
        )
}
