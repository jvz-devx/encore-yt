//! M25: the like mark at the end of a song row (pages and Up next). A
//! liked song shows a filled thumb in `signal` (a toggle that is on),
//! always; any other song shows nothing until the pointer is over its row,
//! then an outline thumb. Clicking it likes or removes the like, through
//! the same change as the player bar's button.

use encore_core::account::AccountAction;
use encore_core::model::{LikeStatus, Track};
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::likes::Likes;
use crate::theme::{Colors, size};

/// The like mark of `track`'s row, whose hover group is `group`. Takes the
/// same room whether or not it shows, so nothing moves when it appears.
pub fn row_like(
    id: SharedString,
    track: &Track,
    likes: &Likes,
    group: impl Into<SharedString>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let liked = likes.liked(track);
    let icon = if liked {
        widgets::glyph(Glyph::ThumbsUp, size::ICON_SM, c.signal)
    } else {
        widgets::icon(IconName::ThumbsUp, size::ICON_SM, c.text_muted)
    };
    let (status, tip) = if liked {
        (LikeStatus::Indifferent, "Remove like")
    } else {
        (LikeStatus::Like, "Like")
    };
    let track = track.clone();
    widgets::icon_button(id, icon, c)
        .when(!liked, |b| {
            b.opacity(0.).group_hover(group, |s| s.opacity(1.))
        })
        .tooltip(widgets::tooltip(tip))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            cx.stop_propagation();
            let track = track.clone();
            this.account_act(AccountAction::Rate { track, status }, cx);
        }))
        .into_any_element()
}

/// The like mark's room in a row that isn't a song (an album or artist
/// row among songs), so the rows' ends line up.
pub fn row_like_space() -> AnyElement {
    div().flex_none().size(size::ICON_BUTTON).into_any_element()
}
