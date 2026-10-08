//! Settings (M24): a wide modal over the window. The category sidebar with
//! the search field sits on the left on `base`, the category on a `surface`
//! panel on the right (header, tabs, its own scrolling body), and a strip
//! of key hints along the foot, so it reads like the window itself. Where
//! it is (category, tabs, search) lives in `crate::settings`.
//!
//! Each category draws its sections in its own file under `settings/`,
//! with [`section`], [`row`] and [`choice`]; a new category is one more
//! file, one more `Category` and one more arm in [`body`].

mod about;
mod account;
mod equalizer;
mod frame;
mod mixes;
mod motion;
mod playback;
mod results;
mod reveal;
mod shortcuts;
mod sidebar;
mod sleep;
pub mod updates;
mod visuals;

use gpui_kit::assets::IconName;
use gpui_kit::base::FocusTrapElement as _;
use gpui_kit::component::input::{Escape, MoveDown, MoveUp};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::keyed::{self, Chip};
use super::overlays::keycap::combo;
use super::widgets;
use crate::app::MusicApp;
use crate::settings::{self as nav, Category};
use crate::theme::{self, Colors, Type, elevation, radius, size, space};

/// The Settings modal while it's open.
pub fn settings(
    app: &MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    if !app.account.settings {
        return None;
    }
    let c = theme::colors(cx);
    let Size {
        width: w,
        height: h,
    } = modal_size(window);
    reveal::begin(&app.settings.scroll);
    let content = if app.settings.query.is_empty() {
        frame::content(app, &c, window, cx)
    } else {
        results::content(app, &c, cx)
    };
    // The focus sits on the panel, inside both key contexts: single-key
    // shortcuts stay out (`MusicDialog`), Settings' own keys work.
    let panel = keys(app, cx)
        .id("settings")
        .debug_selector(|| "settings".into())
        .track_focus(&app.account.focus)
        .w(w)
        .h(h)
        .flex()
        .flex_col()
        .rounded(radius::LG)
        .overflow_hidden()
        .bg(c.base)
        .border_1()
        .border_color(c.outline)
        .shadow(elevation::high(&c))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(sidebar::sidebar(app, &c, cx))
                .child(content),
        )
        .child(frame::footer(&c))
        .focus_trap("settings-trap", &app.account.focus);
    let panel = div()
        .key_context(crate::account::DIALOG_CONTEXT)
        .child(panel);
    Some(
        widgets::scrim("settings-scrim", &c)
            .on_click(cx.listener(|this, _, window, cx| this.open_settings(false, window, cx)))
            .child(widgets::settle_in(
                "settings-in",
                theme::motion::Kind::Panels,
                panel,
            ))
            .into_any_element(),
    )
}

/// Most of the window, with comfortable margins (a little narrower in a
/// small window), up to `SETTINGS_W` by `SETTINGS_H`.
fn modal_size(window: &Window) -> Size<Pixels> {
    let viewport = window.viewport_size();
    let margin = if viewport.width < px(1100.) {
        space::XL
    } else {
        space::XXXL
    };
    size(
        size::SETTINGS_W.min(viewport.width - margin * 2.),
        size::SETTINGS_H.min(viewport.height - margin * 2.),
    )
}

/// The Settings key context and its keys: ↑/↓ change the category (or
/// walk the search results), Ctrl+Tab and Ctrl+PageDown the tab, `/`
/// searches, Esc clears the search before it closes.
fn keys(app: &MusicApp, cx: &mut Context<MusicApp>) -> Div {
    let searching = !app.settings.query.is_empty();
    let count = if searching {
        nav::search(&app.settings.query).len()
    } else {
        0
    };
    let category = app.settings.category;
    let tabs = if category == Category::Visuals && !visuals::tabs_shown() {
        0
    } else {
        category.tabs().len()
    };
    div()
        .key_context(nav::CONTEXT)
        .on_action(
            cx.listener(move |this, _: &nav::NextTab, window, cx| {
                this.step_tab(1, tabs, window, cx)
            }),
        )
        .on_action(cx.listener(move |this, _: &nav::PreviousTab, window, cx| {
            this.step_tab(-1, tabs, window, cx)
        }))
        .on_action(
            cx.listener(|this, _: &nav::NextCategory, window, cx| {
                this.step_category(1, window, cx)
            }),
        )
        .on_action(cx.listener(|this, _: &nav::PreviousCategory, window, cx| {
            this.step_category(-1, window, cx)
        }))
        .on_action(cx.listener(|this, _: &nav::FocusSearch, window, cx| {
            this.focus_settings_search(window, cx)
        }))
        // Before the field handles them: Esc clears it, the arrows walk the
        // results (or the categories while it is empty).
        .capture_action(cx.listener(|this, _: &Escape, window, cx| {
            this.settings_escape(window, cx);
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(move |this, _: &MoveUp, window, cx| {
            if searching {
                this.move_settings_hit(-1, count, cx);
            } else {
                this.step_category(-1, window, cx);
            }
            cx.stop_propagation();
        }))
        .capture_action(cx.listener(move |this, _: &MoveDown, window, cx| {
            if searching {
                this.move_settings_hit(1, count, cx);
            } else {
                this.step_category(1, window, cx);
            }
            cx.stop_propagation();
        }))
}

/// The category's sections, for the scrolling body.
fn body(
    app: &MusicApp,
    category: Category,
    c: &Colors,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Vec<AnyElement> {
    let tab = app.settings.tab(category);
    match category {
        Category::Account => account::page(app, c, cx),
        Category::Playback => playback::page(app, c, window, cx),
        Category::Equalizer => equalizer::page(app, c, cx),
        Category::Visuals => vec![visuals::view(app, c, window, cx)],
        Category::Motion => motion::page(tab, c, cx),
        Category::Shortcuts => shortcuts::page(tab, c),
        Category::Updates => updates::page(app, c, cx),
        Category::About => about::page(app, c, cx),
    }
}

/// A Reset for the category (or its tab) once it differs from the
/// defaults; Visuals has its own per tab.
fn reset(
    app: &MusicApp,
    category: Category,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    let tab = app.settings.tab(category);
    let changed = match category {
        Category::Playback => playback::changed(app),
        Category::Equalizer => equalizer::changed(app),
        Category::Motion => motion::changed(tab),
        _ => false,
    };
    changed.then(|| {
        focusable(
            widgets::pill_button(
                "settings-reset",
                "Reset",
                Some(widgets::icon(IconName::RotateCcw, size::ICON_SM, c.text)),
                widgets::Pill::Secondary,
                c,
            )
            .debug_selector(|| "settings-reset".into()),
            c,
        )
        .tooltip(widgets::tooltip("Back to the defaults"))
        .on_click(cx.listener(move |this, _, _, cx| match category {
            Category::Playback => playback::reset(this, cx),
            Category::Equalizer => equalizer::reset(this, cx),
            Category::Motion => motion::reset(tab, cx),
            _ => {}
        }))
        .into_any_element()
    })
}

/// A group of settings: an optional name over a card holding its rows.
fn section(
    name: &'static str,
    c: &Colors,
    rows: impl IntoIterator<Item = AnyElement>,
) -> AnyElement {
    v_flex()
        .gap(space::SM)
        .when(!name.is_empty(), |s| {
            s.child(div().type_label().text_color(c.text_muted).child(name))
        })
        .child(
            v_flex()
                .rounded(radius::MD)
                .bg(c.hover)
                .px(space::LG)
                .py(space::XS)
                .children(rows.into_iter().map(reveal::wrap)),
        )
        .into_any_element()
}

/// A setting: what it is, a line saying what it does or what it is now,
/// and its control at the right.
fn row(
    label: impl Into<SharedString>,
    detail: Option<SharedString>,
    control: impl IntoElement,
    c: &Colors,
) -> AnyElement {
    keyed_row(label, &[], detail, control, c)
}

/// A setting with a shortcut: its keycaps after the label.
fn keyed_row(
    label: impl Into<SharedString>,
    keys: &[&'static str],
    detail: Option<SharedString>,
    control: impl IntoElement,
    c: &Colors,
) -> AnyElement {
    let title = h_flex()
        .gap(space::SM)
        .child(
            div()
                .min_w_0()
                .truncate()
                .type_body()
                .tabular()
                .child(label.into()),
        )
        .when(!keys.is_empty(), |t| t.child(combo(keys, c)));
    h_flex()
        .py(space::SM + space::XXS)
        .gap(space::LG)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(title)
                .children(detail.map(|d| {
                    div()
                        .type_small()
                        .tabular()
                        .text_color(c.text_muted)
                        .child(d)
                })),
        )
        .child(control)
        .into_any_element()
}

/// A button with its shortcut beside it ("Adjust bands  E").
fn keyed(button: impl IntoElement, keys: &[&'static str], c: &Colors) -> AnyElement {
    h_flex()
        .gap(space::SM)
        .child(button)
        .child(combo(keys, c))
        .into_any_element()
}

/// One of a set of choices (an equalizer preset, a sleep timer): a pill
/// tinted over its card (as `raised` is over the page), or `primary` while
/// it is the one in effect, as YouTube Music draws its filter chips. Tab
/// reaches it, Enter or Space picks it, and in [`choices`] ←/→ pick the one
/// beside it (M29).
fn choice(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    chosen: bool,
    c: &Colors,
) -> Chip {
    let (bg, hover, fg) = if chosen {
        (c.primary, c.primary_hover, c.primary_foreground)
    } else {
        (c.hover, c.pressed, c.text)
    };
    let label: SharedString = label.into();
    let name: SharedString = format!("settings-choice:{label}").into();
    let chip = div()
        .id(id)
        .debug_selector(move || name.to_string())
        .flex_none()
        .h(size::CHIP)
        .px(space::LG)
        .flex()
        .items_center()
        .rounded(radius::FULL)
        .bg(bg)
        .text_color(fg)
        .type_label()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(|s| s.opacity(0.9))
        .child(label);
    // The chosen chip's fill is opaque: its ring can stand outside it.
    Chip::new(if chosen {
        keyed::ring_outside(chip, c)
    } else {
        keyed::ring_inside(chip, c)
    })
}

/// A wrapping row of [`choice`]s under a setting; ←/→ move the choice.
fn choices(chips: impl IntoIterator<Item = Chip>) -> AnyElement {
    h_flex()
        .flex_wrap()
        .gap(space::SM)
        .pb(space::SM)
        .children(keyed::arrow_row(chips))
        .into_any_element()
}

/// Makes a control a tab stop with a ring while the keyboard is on it,
/// inside its edge (M29: a ring outside showed as a light fill on rows and
/// buttons without an opaque fill).
fn focusable(el: Stateful<Div>, c: &Colors) -> Stateful<Div> {
    keyed::ring_inside(el, c)
}
