//! The light look's scrim behind text over a 3D scene (M30): which text
//! blocks the scene keeps clear, from the views' slots, in the scene's
//! points. The scene's shader draws it (`encore_visuals::Scrim`): the
//! mask costs a few operations a pixel in a pass that runs anyway, it
//! adapts to the scene under it (each pixel lifted only as far as text
//! needs, its hue kept), and it adds no layer to the window's frames.

use encore_visuals::{Look, Scrim};
use gpui_kit::*;

use super::config;
use super::slots::{self, Slot, Slots};
use super::visualizer::Place;
use crate::theme;

/// The scrim for a scene filling `region` (window coordinates) at
/// `place`: off in the dark look and when the setting keeps the old
/// whole-scene tone.
pub(super) fn scene_scrim(place: Place, region: Bounds<Pixels>, look: Look, cx: &App) -> Scrim {
    let scenes = &config::get().scenes;
    if look != Look::Light || !scenes.scrim {
        return Scrim::default();
    }
    let surface = theme::colors(cx).surface.to_rgb();
    let mut scrim = Scrim {
        on: true,
        strength: scenes.scrim_strength,
        size: [f32::from(region.size.width), f32::from(region.size.height)],
        surface: [surface.r, surface.g, surface.b],
        ..Scrim::default()
    };
    for block in text_blocks(place, cx) {
        let b = block.intersect(&region);
        let o = b.origin - region.origin;
        scrim.add([
            f32::from(o.x),
            f32::from(o.y),
            f32::from(o.x + b.size.width),
            f32::from(o.y + b.size.height),
        ]);
    }
    scrim
}

/// Where text sits over the scene at `place`, in window coordinates.
fn text_blocks(place: Place, cx: &App) -> Vec<Bounds<Pixels>> {
    let get = |slot| Slots::get(cx, slot);
    match place {
        Place::NowPlaying => {
            // The top bar (Back, search, the account) over the panel's top.
            let top = slots::panel(cx).zip(get(Slot::Page)).map(|(panel, page)| {
                Bounds::from_corners(panel.origin, point(panel.right(), page.top()))
            });
            [get(Slot::SongText), get(Slot::Tabs), top]
                .into_iter()
                .flatten()
                .collect()
        }
        Place::Stage => {
            // The transport along the bottom, under the body.
            let transport = get(Slot::Stage)
                .zip(get(Slot::StageBody))
                .map(|(stage, body)| {
                    Bounds::from_corners(point(stage.left(), body.bottom()), stage.bottom_right())
                });
            [
                get(Slot::Title),
                get(Slot::Lyrics),
                get(Slot::Corner),
                transport,
            ]
            .into_iter()
            .flatten()
            .collect()
        }
        Place::Full => [get(Slot::Title), get(Slot::Corner)]
            .into_iter()
            .flatten()
            .collect(),
    }
}
