//! Up next: the queue in play order, as a panel beside the page and as Now
//! Playing's first tab. Rows play on click, drag to reorder and remove on
//! hover; the list follows the current song.

mod row;

use gpui_kit::assets::IconName;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use ytfast::backend::Command;

use super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::playback::QueueScroll;
use crate::theme::{self, Colors, Type, radius, size, space};

/// The panel's width.
const PANEL: Pixels = px(360.);

/// Which list a row belongs to: the side panel or Now Playing's tab.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Panel,
    Tab,
}

impl Place {
    fn scroll(self, app: &mut MusicApp) -> &mut QueueScroll {
        match self {
            Place::Panel => &mut app.player.panel_scroll,
            Place::Tab => &mut app.player.tab_scroll,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Place::Panel => "panel",
            Place::Tab => "tab",
        }
    }
}

/// The Up next panel beside the page, while it's open (Now Playing shows
/// the queue in its own tab instead).
pub fn panel(
    app: &mut MusicApp,
    _window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    if !app.player.queue_open || app.player.now_playing {
        return None;
    }
    let c = theme::colors(cx);
    Some(
        v_flex()
            .w(PANEL)
            .flex_none()
            .h_full()
            .mt(space::SM)
            .mr(space::SM)
            .mb(space::XS)
            .rounded(radius::LG)
            .bg(c.surface)
            .overflow_hidden()
            .child(
                h_flex()
                    .h(size::TOP_BAR)
                    .flex_none()
                    .pl(space::XL)
                    .pr(space::MD)
                    .gap(space::SM)
                    .child(div().flex_1().type_heading().child("Up next"))
                    .children(super::account::save_queue_button(app, &c, cx))
                    .children(clear_button(app, &c, cx))
                    .child(
                        widgets::icon_button(
                            "close-up-next",
                            widgets::icon(IconName::X, size::ICON, c.text_muted),
                            &c,
                        )
                        .tooltip(widgets::tooltip("Close"))
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_up_next(cx))),
                    ),
            )
            .child(controls(app, &c, cx).px(space::XL).pb(space::MD))
            .child(list(app, Place::Panel, &c, cx).px(space::SM).pb(space::SM))
            .with_animation(
                "up-next-open",
                Animation::new(theme::motion::BASE).with_easing(theme::motion::ease_out),
                |el, t| el.opacity(t),
            )
            .into_any_element(),
    )
}

/// Clear: removes the songs after the current one, while there are any.
pub fn clear_button(
    app: &MusicApp,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> Option<Stateful<Div>> {
    let upcoming = app
        .player
        .playback
        .index
        .is_some_and(|i| i + 1 < app.player.queue.len());
    upcoming.then(|| {
        widgets::pill_button(
            "clear-queue",
            "Clear",
            Some(widgets::icon(IconName::ListX, size::ICON_SM, c.text)),
            Pill::Secondary,
            c,
        )
        .tooltip(widgets::tooltip("Remove the songs after this one"))
        .on_click(cx.listener(|this, _, _, cx| this.edit_queue(Command::ClearUpcoming, cx)))
    })
}

/// The Autoplay switch, above the list.
pub fn controls(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Div {
    let autoplay = app.player.playback.autoplay;
    h_flex()
        .flex_none()
        .gap(space::MD)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().type_label().child("Autoplay"))
                .child(widgets::muted_line(
                    "Adds similar songs when the queue ends",
                    c,
                )),
        )
        .child(
            Switch::new("autoplay-switch")
                .checked(autoplay)
                .color(c.signal)
                .on_click(cx.listener(|this, on: &bool, _, cx| {
                    this.edit_queue(Command::Autoplay(*on), cx)
                })),
        )
}

/// The queue as a virtual list of rows, scrolled to the current song when
/// it changes.
pub fn list(app: &mut MusicApp, place: Place, c: &Colors, cx: &mut Context<MusicApp>) -> Div {
    let count = app.player.queue.len();
    if count == 0 {
        return v_flex()
            .flex_1()
            .justify_center()
            .child(widgets::empty_state(
                IconName::ListMusic,
                "Nothing in the queue",
                "Play a song, an album or a radio and what comes next shows here.",
                c,
            ));
    }
    let current = app.player.playback.index;
    let scroll = place.scroll(app);
    if current.is_some() && scroll.followed != current {
        scroll.followed = current;
        if let Some(i) = current {
            scroll.handle.scroll_to_item(i, ScrollStrategy::Center);
        }
    }
    let handle = scroll.handle.clone();
    let colors = *c;
    v_flex().flex_1().min_h_0().child(
        uniform_list(
            SharedString::from(format!("queue-{}", place.name())),
            count,
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .filter_map(|i| row::row(this, place, i, &colors, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&handle)
        .size_full(),
    )
}
