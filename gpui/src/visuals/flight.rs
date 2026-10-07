//! The cover flying between the player bar and Now Playing, over the app.
//!
//! Opening Now Playing lifts a copy of the cover from the player bar into
//! the large cover's place (the real one stays hidden until it lands);
//! closing flies it back down over the page. It runs for `motion::SLOW`
//! with `motion::ease_out`, drawn by this view alone, and not at all under
//! reduced motion.

use std::time::{Duration, Instant};

use gpui_kit::*;

use super::content::Content;
use super::slots::{Slot, Slots};
use crate::theme::{self, elevation, motion, radius, size, space};

struct Trip {
    opening: bool,
    started: Instant,
    /// Closing: where the large cover was.
    from: Option<Bounds<Pixels>>,
}

pub struct Flight {
    content: Entity<Content>,
    showing: Option<bool>,
    trip: Option<Trip>,
    _landed: Option<Task<()>>,
}

impl Flight {
    pub fn new(content: Entity<Content>) -> Self {
        Self {
            content,
            showing: None,
            trip: None,
            _landed: None,
        }
    }

    /// Starts a trip when Now Playing opens or closes; true while one runs.
    pub fn follow(&mut self, showing: bool, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        let was = self.showing.replace(showing);
        let on = super::config::get().flight.on;
        if was.is_some_and(|was| was != showing) && on && !super::reduced_motion(cx) {
            let from = Slots::get(cx, Slot::Cover);
            if showing || from.is_some() {
                self.trip = Some(Trip {
                    opening: showing,
                    started: Instant::now(),
                    from,
                });
                self._landed = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(duration()).await;
                    let _ = this.update(cx, |this, cx| this.land(cx));
                }));
                cx.notify();
            }
        }
        self.trip.is_some()
    }

    /// The copy is on its way into Now Playing: the real cover waits.
    pub fn landing(&self) -> bool {
        self.trip.as_ref().is_some_and(|t| t.opening)
    }

    fn land(&mut self, cx: &mut Context<Self>) {
        self.trip = None;
        self._landed = None;
        // The app view shows its own cover again.
        self.content.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}

impl Render for Flight {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layer = div().absolute().inset_0();
        let Some(trip) = &self.trip else {
            return layer;
        };
        let (opening, started, from) = (trip.opening, trip.started, trip.from);
        let covers = Slots::covers(cx);
        let shadows = elevation::high(&theme::colors(cx));
        layer.child(
            canvas(
                |_, _, _| (),
                move |_, (), window, cx| {
                    let t = (started.elapsed().as_secs_f32() / duration().as_secs_f32()).min(1.);
                    let bar = player_cover(window.viewport_size());
                    let (from, to) = if opening {
                        (Some(bar), Slots::get(cx, Slot::Cover))
                    } else {
                        (from, Some(bar))
                    };
                    if let (Some(from), Some(to)) = (from, to) {
                        let e = motion::ease_out(t);
                        let (start_radius, end_radius) = if opening {
                            (radius::SM, radius::LG)
                        } else {
                            (radius::LG, radius::SM)
                        };
                        let corners = Corners::all(start_radius + (end_radius - start_radius) * e);
                        let bounds = lerp(from, to, e);
                        let image = [covers.large.clone(), covers.small.clone()]
                            .into_iter()
                            .flatten()
                            .find_map(|url| {
                                let resource = Resource::Uri(SharedUri::from(url));
                                window.use_asset::<ImgResourceLoader>(&resource, cx)?.ok()
                            });
                        window.paint_drop_shadows(bounds, corners, &shadows);
                        if let Some(image) = image {
                            let _ = window.paint_image(bounds, bounds, corners, image, 0, false);
                        }
                    }
                    if t < 1. {
                        window.request_animation_frame();
                    }
                },
            )
            .size_full(),
        )
    }
}

/// How long a trip takes: Settings → Visuals (`motion::SLOW` by default),
/// or `YTFAST_GPUI_VISUALS_FLIGHT_MS` (to look at it in slow motion).
fn duration() -> Duration {
    Duration::from_millis(u64::from(super::config::get().flight.ms))
}

/// The player bar's cover: `space::LG` from the left, centred in the bar.
fn player_cover(viewport: Size<Pixels>) -> Bounds<Pixels> {
    let top = viewport.height - size::PLAYER_BAR + (size::PLAYER_BAR - size::PLAYER_COVER) / 2.;
    Bounds::new(
        point(space::LG, top),
        gpui_kit::size(size::PLAYER_COVER, size::PLAYER_COVER),
    )
}

fn lerp(from: Bounds<Pixels>, to: Bounds<Pixels>, t: f32) -> Bounds<Pixels> {
    let mix = |a: Pixels, b: Pixels| a + (b - a) * t;
    Bounds::new(
        point(mix(from.left(), to.left()), mix(from.top(), to.top())),
        gpui_kit::size(
            mix(from.size.width, to.size.width),
            mix(from.size.height, to.size.height),
        ),
    )
}
