//! M4: layers over the window, bottom to top: a short note above the player
//! bar ("Link copied"), the sleep timer and equalizer panels, the shortcuts
//! sheet (`?`), Play anything (Ctrl+K) and a context menu. Drawn after the
//! rest of the window.

mod keycap;
mod palette;
mod shortcuts;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::{menu, widgets};
use crate::app::MusicApp;
use crate::theme::motion::MotionExt as _;
use crate::theme::{self, Colors, Type, elevation, motion, radius, size, space};

pub fn overlays(
    app: &mut MusicApp,
    window: &mut Window,
    cx: &mut Context<MusicApp>,
) -> Option<AnyElement> {
    let mut layers: Vec<AnyElement> = Vec::new();
    layers.extend(toast(app, cx));
    // The sleep timer and the equalizer, above the player bar.
    layers.extend(super::extras::panel(app, cx));
    if app.desktop.layers.help {
        layers.push(shortcuts::sheet(window, cx));
    }
    layers.extend(palette::palette(app, window, cx));
    layers.extend(menu::layer(app, cx));
    if layers.is_empty() {
        return None;
    }
    // Above the page and its search list.
    Some(
        deferred(div().absolute().inset_0().children(layers))
            .with_priority(2)
            .into_any_element(),
    )
}

/// A dimmed layer over the whole window that takes the pointer.
fn scrim(id: &'static str, c: &Colors) -> Stateful<Div> {
    v_flex()
        .id(id)
        .absolute()
        .inset_0()
        .items_center()
        .bg(c.scrim)
        .occlude()
}

/// "Link copied", "Playing next": a pill above the player bar for a moment.
fn toast(app: &MusicApp, cx: &mut Context<MusicApp>) -> Option<AnyElement> {
    let (text, stamp, done) = app.desktop.layers.toast.clone()?;
    let c = theme::colors(cx);
    Some(
        h_flex()
            .absolute()
            .left_0()
            .right_0()
            .bottom(size::PLAYER_BAR + space::LG)
            .justify_center()
            .child(
                h_flex()
                    .h(size::CHIP)
                    .pl(if done { space::MD } else { space::LG })
                    .pr(space::LG)
                    .gap(space::SM)
                    .rounded(radius::FULL)
                    .bg(c.overlay)
                    .shadow(elevation::high(&c))
                    .type_label()
                    .children(
                        done.then(|| widgets::icon(IconName::Check, size::ICON_SM, c.success)),
                    )
                    .child(text),
            )
            .with_motion(
                SharedString::from(format!("toast-{stamp}")),
                motion::Kind::Toasts,
                motion::BASE,
                |el, t| {
                    el.opacity(t)
                        .bottom(size::PLAYER_BAR + space::LG - space::SM * (1. - t))
                },
            ),
    )
}
