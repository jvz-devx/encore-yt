//! A page before and around its content: loading (the shape of what is on
//! its way), failed (what happened and Try again), empty, the saved-copy
//! notice, and the search caption.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use ytfast::backend::Command;
use ytfast::model::{Page, Target};

use super::Ctx;
use super::chips::library_tabs;
use crate::app::MusicApp;
use crate::nav::LibraryTab;
use crate::theme::{self, Colors, Type, radius, size, space};
use crate::views::widgets::{self, Pill};

/// What a page that is loading looks like: its tabs if it's the Library,
/// then the shape of its content.
pub fn loading(
    app: &MusicApp,
    target: &Target,
    tab: Option<LibraryTab>,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let c = theme::colors(cx);
    let tabs = tab.map(|tab| {
        let ctx = Ctx::new(app, &target.key(), cx);
        library_tabs(tab, &ctx, cx)
    });
    let shape = match target {
        Target::Search { .. } => v_flex()
            .gap(space::XL)
            .child(widgets::skeleton(px(220.), px(16.), radius::XS, &c))
            .child(widgets::skeleton(
                px(560.),
                size::CARD + space::LG * 2.,
                radius::MD + space::LG,
                &c,
            ))
            .child(skeleton_rows(4, &c))
            .into_any_element(),
        Target::Browse { id, .. } if !id.starts_with("FEmusic_") => v_flex()
            .gap(space::XXL + space::SM)
            .child(skeleton_header(id.starts_with("UC"), &c))
            .child(skeleton_rows(6, &c))
            .into_any_element(),
        _ => v_flex()
            .gap(space::XXL + space::SM)
            .child(skeleton_shelf(&c))
            .child(skeleton_shelf(&c))
            .into_any_element(),
    };
    v_flex()
        .flex_1()
        .w_full()
        .min_h_0()
        .overflow_hidden()
        .px(size::GUTTER)
        .pt(space::SM)
        .gap(space::XL)
        .children(tabs)
        .child(shape)
}

/// A shelf on its way: a title bar and a row of covers.
pub fn skeleton_shelf(c: &Colors) -> impl IntoElement {
    v_flex()
        .gap(space::LG)
        .child(widgets::skeleton(px(200.), px(24.), radius::SM, c))
        .child(
            h_flex()
                .gap(space::XL)
                .items_start()
                .overflow_hidden()
                .children((0..8).map(|_| {
                    v_flex()
                        .gap(space::SM)
                        .child(widgets::skeleton(size::CARD, size::CARD, radius::MD, c))
                        .child(widgets::skeleton(px(140.), px(14.), radius::XS, c))
                        .child(widgets::skeleton(px(96.), px(12.), radius::XS, c))
                })),
        )
}

/// Song rows on their way.
pub fn skeleton_rows(n: usize, c: &Colors) -> impl IntoElement {
    v_flex().w_full().children((0..n).map(|i| {
        h_flex()
            .h(size::ROW)
            .px(space::SM)
            .gap(space::MD)
            .child(widgets::skeleton(
                size::ROW_THUMB,
                size::ROW_THUMB,
                radius::XS,
                c,
            ))
            .child(
                v_flex()
                    .gap(space::XS)
                    .child(widgets::skeleton(
                        px(180. + 40. * (i % 3) as f32),
                        px(14.),
                        radius::XS,
                        c,
                    ))
                    .child(widgets::skeleton(
                        px(120. + 30. * (i % 2) as f32),
                        px(12.),
                        radius::XS,
                        c,
                    )),
            )
    }))
}

/// An album, artist or playlist header on its way.
fn skeleton_header(round: bool, c: &Colors) -> impl IntoElement {
    let corner = if round { radius::FULL } else { radius::LG };
    h_flex()
        .pt(space::LG)
        .gap(space::XXL)
        .items_end()
        .child(widgets::skeleton(
            size::HEADER_COVER,
            size::HEADER_COVER,
            corner,
            c,
        ))
        .child(
            v_flex()
                .gap(space::MD)
                .child(widgets::skeleton(px(320.), px(32.), radius::SM, c))
                .child(widgets::skeleton(px(220.), px(16.), radius::XS, c))
                .child(widgets::skeleton(px(160.), px(14.), radius::XS, c))
                .child(
                    h_flex()
                        .mt(space::SM)
                        .gap(space::SM)
                        .child(widgets::skeleton(px(92.), size::CHIP, radius::FULL, c))
                        .child(widgets::skeleton(px(108.), size::CHIP, radius::FULL, c)),
                ),
        )
}

/// The empty-state and error recipe: a disc with an icon, what happened,
/// a detail, and one button with the fix.
fn notice(
    icon: IconName,
    title: impl Into<SharedString>,
    detail: Option<String>,
    action: Option<Stateful<Div>>,
    c: &Colors,
) -> Div {
    v_flex()
        .w_full()
        .items_center()
        .justify_center()
        .gap(space::MD)
        .py(space::XXXL)
        .child(
            h_flex()
                .size(px(48.))
                .justify_center()
                .rounded(radius::FULL)
                .bg(c.raised)
                .child(widgets::icon(icon, px(22.), c.text_muted)),
        )
        .child(div().type_heading().text_center().child(title.into()))
        .children(detail.map(|d| {
            div()
                .max_w(px(420.))
                .text_center()
                .type_small()
                .text_color(c.text_muted)
                .child(d)
        }))
        .children(action.map(|a| a.mt(space::SM)))
}

/// A page that failed to load: what happened, and Try again.
pub fn failed(
    app: &MusicApp,
    error: String,
    key: &str,
    tab: Option<LibraryTab>,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let c = theme::colors(cx);
    let tabs = tab.map(|tab| {
        let ctx = Ctx::new(app, key, cx);
        library_tabs(tab, &ctx, cx)
    });
    let key = key.to_string();
    let retry = try_again("retry", &c).on_click(cx.listener(move |this, _, _, cx| {
        this.retry_page(&key, cx);
    }));
    v_flex()
        .flex_1()
        .w_full()
        .px(size::GUTTER)
        .pt(space::SM)
        .children(tabs)
        .child(
            notice(
                IconName::CircleAlert,
                "Couldn't load this page",
                Some(error),
                Some(retry),
                &c,
            )
            .flex_1(),
        )
}

fn try_again(id: &'static str, c: &Colors) -> Stateful<Div> {
    widgets::pill_button(
        id,
        "Try again",
        Some(widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text)),
        Pill::Secondary,
        c,
    )
}

/// A page with no shelves: the Library signed out, or YouTube's message.
pub fn empty(app: &MusicApp, page: &Page, ctx: &Ctx, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let library = matches!(app.pages.view, crate::nav::View::Library(_));
    if library && !app.account.signed_in() {
        let reconnect = widgets::pill_button(
            "library-reconnect",
            "Reconnect",
            Some(widgets::icon(IconName::RefreshCw, size::ICON_SM, c.text)),
            Pill::Secondary,
            c,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.send(Command::Reconnect);
            cx.notify();
        }));
        let detail = page.message.clone().unwrap_or_else(|| {
            "Sign in to YouTube Music in your browser, then Reconnect to see your playlists, songs and albums here.".into()
        });
        return notice(
            IconName::LibraryBig,
            "Signed out of YouTube Music",
            Some(detail),
            Some(reconnect),
            c,
        )
        .into_any_element();
    }
    let (title, detail) = match &page.message {
        Some(message) => split_message(message),
        None => ("Nothing here yet".to_string(), None),
    };
    notice(IconName::Music, title, detail, None, c).into_any_element()
}

/// YouTube's message comes as one line, its title and subtext run together
/// ("No songs yet Songs you save…"): split it where a new sentence starts.
fn split_message(message: &str) -> (String, Option<String>) {
    let mut words = message.split(' ').enumerate();
    let at = words.find_map(|(i, w)| {
        (i > 0 && w.chars().next().is_some_and(char::is_uppercase)).then_some(i)
    });
    match at {
        Some(at) => {
            let words: Vec<&str> = message.split(' ').collect();
            (words[..at].join(" "), Some(words[at..].join(" ")))
        }
        None => (message.to_string(), None),
    }
}

/// The saved copy is showing because the refresh failed.
pub fn saved(error: String, ctx: &Ctx, cx: &mut Context<MusicApp>) -> AnyElement {
    let c = &ctx.c;
    let key = ctx.key.clone();
    h_flex()
        .w_full()
        .pl(space::MD)
        .pr(space::XS)
        .py(space::XS)
        .gap(space::MD)
        .rounded(radius::MD)
        .bg(c.raised)
        .type_small()
        .child(widgets::icon(
            IconName::CircleAlert,
            size::ICON_SM,
            c.text_muted,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(c.text_muted)
                .child(format!("Showing the saved copy. {error}")),
        )
        .child(
            try_again("saved-retry", c)
                .h(px(28.))
                .on_click(cx.listener(move |this, _, _, cx| this.retry_page(&key, cx))),
        )
        .into_any_element()
}

/// "Results for “…”" above search results.
pub fn caption(target: &Target, ctx: &Ctx) -> AnyElement {
    let c = &ctx.c;
    let Target::Search { query, .. } = target else {
        return div().into_any_element();
    };
    div()
        .type_body()
        .text_color(c.text_muted)
        .child(format!("Results for “{query}”"))
        .into_any_element()
}
