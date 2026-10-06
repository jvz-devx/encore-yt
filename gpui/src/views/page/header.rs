//! The page header (DESIGN.md "Page header"): cover (round for artists),
//! title, linked subtitle, second subtitle, a description that opens up in
//! full, and Play, Shuffle, Radio and the account's actions.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use ytfast::model::{Header, Target};

use super::Ctx;
use super::runs::runs_line;
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::theme::{Type, size, space};
use crate::views::widgets::{self, Pill};

/// A description longer than this may run past three lines, so it gets
/// More.
const LONG_DESCRIPTION: usize = 260;
/// Lines of the description shown until More.
const DESCRIPTION_LINES: usize = 3;

pub fn header(
    app: &MusicApp,
    header: &Header,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let c = &ctx.c;
    let plain = header.thumbnail.is_none() && header.play.is_none() && header.subtitle.is_empty();
    let title = div()
        .type_display()
        .line_clamp(2)
        .child(header.title.clone());
    if plain {
        // A titled list (a mood, a chart): just its name.
        return title.into_any_element();
    }
    let has_songs = app
        .pages
        .states
        .get(&ctx.key)
        .and_then(|s| s.page.as_ref())
        .is_some_and(|p| {
            p.shelves
                .iter()
                .any(|s| s.items.iter().any(|i| i.track.is_some()))
        });
    let key = ctx.key.clone();
    let play = (header.play.is_some() || has_songs).then(|| {
        widgets::pill_button(
            "header-play",
            "Play",
            Some(widgets::glyph(
                Glyph::Play,
                size::ICON_SM,
                c.primary_foreground,
            )),
            Pill::Primary,
            c,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.play_page(&key, cx)))
    });
    let secondary = |id: &'static str, label: &'static str, icon: IconName, target: Target| {
        widgets::pill_button(
            id,
            label,
            Some(widgets::icon(icon, size::ICON_SM, c.text)),
            Pill::Secondary,
            c,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.activate(target.clone(), cx)))
    };
    let shuffle = header
        .shuffle
        .clone()
        .map(|t| secondary("header-shuffle", "Shuffle", IconName::Shuffle, t));
    let radio = header
        .radio
        .clone()
        .map(|t| secondary("header-radio", "Radio", IconName::Radio, t));
    let actions = h_flex()
        .mt(space::LG)
        .gap(space::SM)
        .children(play)
        .children(shuffle)
        .children(radio)
        .children(crate::views::account::header_actions(app, header, cx));
    h_flex()
        .w_full()
        .pt(space::LG)
        .gap(space::XXL)
        .items_end()
        .child(widgets::cover(
            header.thumbnail.clone().map(Into::into),
            size::HEADER_COVER,
            header.round,
            c,
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XS)
                .child(title)
                .when(!header.subtitle.is_empty(), |col| {
                    col.child(div().mt(space::XS).type_body().child(runs_line(
                        ctx.id("header-sub"),
                        &header.subtitle,
                        c.text_muted,
                        ctx.link.as_ref(),
                        c,
                        cx,
                    )))
                })
                .when(!header.second_subtitle.is_empty(), |col| {
                    col.child(
                        div()
                            .type_small()
                            .tabular()
                            .text_color(c.text_muted)
                            .child(header.second_subtitle.clone()),
                    )
                })
                .children(
                    header
                        .description
                        .clone()
                        .map(|d| description(d, app.pages.expanded.contains(&ctx.key), ctx, cx)),
                )
                .child(actions),
        )
        .into_any_element()
}

/// Three lines of the description, or all of it; More / Less when it is
/// long.
fn description(
    text: String,
    expanded: bool,
    ctx: &Ctx,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let c = &ctx.c;
    let long = text.chars().count() > LONG_DESCRIPTION || text.contains('\n');
    // Clamped, the paragraphs run on: GPUI clamps each paragraph on its own.
    let text = if expanded {
        text
    } else {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let key = ctx.key.clone();
    v_flex()
        .mt(space::SM)
        .w_full()
        .max_w(px(640.))
        .items_start()
        .gap(space::XXS)
        .child(
            // Full width: in a column that doesn't stretch its children,
            // text would take its whole length and run past the edge.
            div()
                .w_full()
                .type_small()
                .text_color(c.text_muted)
                .when(!expanded, |d| {
                    d.line_clamp(DESCRIPTION_LINES).text_ellipsis()
                })
                .child(text),
        )
        .when(long, |col| {
            col.child(
                div()
                    .id("header-description-toggle")
                    .type_caption()
                    .text_color(c.text)
                    .cursor_pointer()
                    .hover(|s| s.underline())
                    .child(if expanded { "Less" } else { "More" })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_description(&key, cx))),
            )
        })
}
