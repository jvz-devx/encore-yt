//! Hover intent (M28): cards, rows and Play buttons report the pointer
//! over them, and `pages::prefetch` acts once it rests there. Song rows and
//! song cards report through audition's listener (`views::extras::audition`),
//! which already holds their hover.

use gpui_kit::*;
use ytfast::model::Item;

use crate::app::MusicApp;
use crate::pages::Want;

/// A card or row that opens a page: resting on it loads that page.
pub fn page(el: Stateful<Div>, item: &Item, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    match Want::page(item) {
        Some(want) => report(el, want, cx),
        None => el,
    }
}

/// A Play button: resting on it resolves the song it starts with. A song's
/// own card reports that already.
pub fn play(el: Stateful<Div>, item: &Item, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    match Want::play(item).filter(|_| item.track.is_none()) {
        Some(want) => report(el, want, cx),
        None => el,
    }
}

/// Reports the pointer over `el` as interest in `want`.
pub fn report(el: Stateful<Div>, want: Want, cx: &mut Context<MusicApp>) -> Stateful<Div> {
    el.on_hover(cx.listener(move |this, on: &bool, _, cx| this.hover_intent(want.clone(), *on, cx)))
}
