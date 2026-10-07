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
    for (block, feather) in text_blocks(place, cx) {
        let b = block.intersect(&region);
        if b.size.width <= px(0.) || b.size.height <= px(0.) {
            continue;
        }
        let o = b.origin - region.origin;
        scrim.add(
            to_edges(
                [
                    f32::from(o.x),
                    f32::from(o.y),
                    f32::from(o.x + b.size.width),
                    f32::from(o.y + b.size.height),
                ],
                scrim.size,
            ),
            feather,
        );
    }
    scrim
}

/// A block this close to the scene's edge (points) reaches past it ...
const EDGE: f32 = 64.;
/// ... by this much, past the mask's fall: the top bar, the corner
/// buttons and the transport then get light from the edge, like a
/// vignette, instead of a cloud standing in the scene.
const PAST_EDGE: f32 = 400.;

fn to_edges(mut b: [f32; 4], size: [f32; 2]) -> [f32; 4] {
    if b[0] < EDGE {
        b[0] = -PAST_EDGE;
    }
    if b[1] < EDGE {
        b[1] = -PAST_EDGE;
    }
    if b[2] > size[0] - EDGE {
        b[2] = size[0] + PAST_EDGE;
    }
    if b[3] > size[1] - EDGE {
        b[3] = size[1] + PAST_EDGE;
    }
    b
}

/// How far the light falls off round a block, in points.
/// A lone title or song: wide, so it reads as light round the words.
const LONE: f32 = 200.;
/// Lyrics, the top bar, the transport: a band of text.
const BAND: f32 = 120.;
/// Now Playing's tab column draws its own glass panel: the light keeps
/// close to its edge.
const PANEL: f32 = 36.;

/// Where text sits over the scene at `place`, in window coordinates, with
/// how far its light falls off.
fn text_blocks(place: Place, cx: &App) -> Vec<(Bounds<Pixels>, f32)> {
    let get = |slot| Slots::get(cx, slot);
    let blocks: Vec<(Option<Bounds<Pixels>>, f32)> = match place {
        Place::NowPlaying => {
            // The top bar (Back, search, the account) over the panel's top.
            let top = slots::panel(cx).zip(get(Slot::Page)).map(|(panel, page)| {
                Bounds::from_corners(panel.origin, point(panel.right(), page.top()))
            });
            vec![
                (get(Slot::SongText), LONE),
                (get(Slot::Tabs), PANEL),
                (top, BAND),
            ]
        }
        Place::Stage => {
            // The transport along the bottom, under the body.
            let transport = get(Slot::Stage)
                .zip(get(Slot::StageBody))
                .map(|(stage, body)| {
                    Bounds::from_corners(point(stage.left(), body.bottom()), stage.bottom_right())
                });
            vec![
                (get(Slot::Title), LONE),
                (get(Slot::Lyrics), BAND),
                (get(Slot::Corner), LONE),
                (transport, BAND),
            ]
        }
        Place::Full => vec![(get(Slot::Title), LONE), (get(Slot::Corner), BAND)],
    };
    blocks
        .into_iter()
        .filter_map(|(b, feather)| b.map(|b| (b, feather)))
        .collect()
}
