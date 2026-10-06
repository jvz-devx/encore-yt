//! The equalizer's bands as one smooth curve over a ±12 dB field: each band
//! a knob on the curve, dragged up or down (a click sets it, a double click
//! resets it). The curve is the sound's shape, so it is drawn in signal
//! while the equalizer is on.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;
use ytfast::equalizer::{BANDS, Equalizer, RANGE};

use crate::app::MusicApp;
use crate::theme::{Colors, Type, radius, space};

/// The field's height, and the room kept above +12 and below −12 so knobs
/// at the ends stay whole.
const HEIGHT: Pixels = px(176.);
const PAD: f32 = 12.;
/// The scale's column on the left.
const SCALE: Pixels = px(34.);
const KNOB: f32 = 14.;
const KNOB_ACTIVE: f32 = 18.;

/// A band being dragged (the drag's value; it draws nothing).
#[derive(Clone)]
struct BandDrag;

impl Render for BandDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub fn graph(
    app: &MusicApp,
    eq: &Equalizer,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let active = app.extras.eq_band.or(app.extras.eq_hover);
    v_flex()
        .gap(space::SM)
        .child(
            h_flex()
                .gap(space::SM)
                .child(scale(c))
                .child(field(app, eq, active, c, cx)),
        )
        .child(
            h_flex()
                .pl(SCALE + space::SM)
                .children((0..BANDS.len()).map(|i| {
                    div()
                        .flex_1()
                        .text_center()
                        .type_caption()
                        .tabular()
                        .text_color(if active == Some(i) {
                            c.text
                        } else {
                            c.text_faint
                        })
                        .child(short_label(i))
                })),
        )
}

/// "+12", "0", "−12" beside the field.
fn scale(c: &Colors) -> impl IntoElement {
    let label = |t: &'static str| {
        div()
            .h(px(16.))
            .text_right()
            .type_caption()
            .tabular()
            .text_color(c.text_faint)
            .child(t)
    };
    v_flex()
        .w(SCALE)
        .h(HEIGHT)
        .py(px(PAD - 8.))
        .justify_between()
        .child(label("+12"))
        .child(label("0"))
        .child(label("−12"))
}

/// Where gain `g` sits in a field `h` tall.
fn y_of(g: f32, h: f32) -> f32 {
    PAD + (RANGE - g) / (2. * RANGE) * (h - 2. * PAD)
}

/// The gain at height `y` in a field `h` tall.
fn gain_at(y: f32, h: f32) -> f32 {
    RANGE - (y - PAD) / (h - 2. * PAD) * 2. * RANGE
}

fn field(
    app: &MusicApp,
    eq: &Equalizer,
    active: Option<usize>,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement {
    let gains = eq.gains;
    let on = eq.enabled;
    let colors = *c;
    let store = app.extras.eq_bounds.clone();
    let value = active.map(|i| value_label(i, gains[i], c));
    div()
        .id("eq-field")
        .relative()
        .flex_1()
        .h(HEIGHT)
        .cursor(CursorStyle::ResizeUpDown)
        .child(
            canvas(
                move |bounds, _, _| store.set(bounds),
                move |bounds, _, window, _| paint(bounds, &gains, on, active, &colors, window),
            )
            .size_full(),
        )
        .children(value)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, e: &MouseDownEvent, _, cx| {
                let Some((band, gain)) = this.band_at(e.position) else {
                    return;
                };
                let gain = if e.click_count >= 2 { 0.0 } else { gain };
                this.extras.eq_band = Some(band);
                let eq = this.equalizer().with_band(band, gain);
                this.set_equalizer(eq, cx);
            }),
        )
        .on_drag(BandDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
        .on_drag_move(cx.listener(|this, e: &DragMoveEvent<BandDrag>, _, cx| {
            let (Some(band), Some((_, gain))) =
                (this.extras.eq_band, this.band_at(e.event.position))
            else {
                return;
            };
            let eq = this.equalizer().with_band(band, gain);
            this.set_equalizer(eq, cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.end_band_drag(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.end_band_drag(cx)),
        )
        .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
            let band = this.band_at(e.position).map(|(b, _)| b);
            if this.extras.eq_hover != band {
                this.extras.eq_hover = band;
                cx.notify();
            }
        }))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !hovered {
                this.extras.eq_hover = None;
                cx.notify();
            }
        }))
}

impl MusicApp {
    /// The band under `at` and the gain its height asks for.
    fn band_at(&self, at: Point<Pixels>) -> Option<(usize, f32)> {
        let b = self.extras.eq_bounds.get();
        if b.size.width <= px(0.) {
            return None;
        }
        let x = f32::from((at.x - b.left()) / b.size.width);
        let band = ((x * BANDS.len() as f32).floor() as isize).clamp(0, BANDS.len() as isize - 1);
        let y = f32::from(at.y - b.top());
        let gain = gain_at(y, f32::from(b.size.height)).clamp(-RANGE, RANGE);
        Some((band as usize, gain))
    }

    fn end_band_drag(&mut self, cx: &mut Context<Self>) {
        if self.extras.eq_band.take().is_some() {
            cx.notify();
        }
    }
}

/// The band's value over its knob while it is under the pointer or held.
fn value_label(band: usize, gain: f32, c: &Colors) -> impl IntoElement {
    let at = (band as f32 + 0.5) / BANDS.len() as f32;
    let y = y_of(gain, f32::from(HEIGHT));
    // Above the knob, or below it near the top.
    let top = if y < 40. { y + 14. } else { y - 38. };
    h_flex()
        .absolute()
        .left(relative(at))
        .ml(px(-28.))
        .top(px(top))
        .w(px(56.))
        .h(px(24.))
        .justify_center()
        .rounded(radius::SM)
        .bg(c.text)
        .text_color(c.overlay)
        .type_caption()
        .tabular()
        .child(if gain.abs() < 0.05 {
            "0 dB".to_string()
        } else {
            format!("{}{:.1} dB", if gain > 0. { "+" } else { "−" }, gain.abs())
        })
}

/// The field: guide lines, the curve through the knobs with the area it
/// lifts or cuts, and the knobs.
fn paint(
    bounds: Bounds<Pixels>,
    gains: &[f32; 10],
    on: bool,
    active: Option<usize>,
    c: &Colors,
    window: &mut Window,
) {
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    let left = f32::from(bounds.left());
    let top = f32::from(bounds.top());
    let pt = |x: f32, y: f32| point(px(left + x), px(top + y));
    let zero = y_of(0., h);
    let column = w / BANDS.len() as f32;
    let knobs: Vec<(f32, f32)> = gains
        .iter()
        .enumerate()
        .map(|(i, g)| ((i as f32 + 0.5) * column, y_of(*g, h)))
        .collect();

    // Guides: each band's track, the ±12 edges faint, 0 dB stronger.
    for (x, _) in &knobs {
        window.paint_quad(fill(
            Bounds::new(pt(x - 0.5, PAD), size(px(1.), px(h - 2. * PAD))),
            c.hairline,
        ));
    }
    for (y, color) in [
        (PAD, c.hairline),
        (h - PAD, c.hairline),
        (zero, c.text_faint.opacity(0.45)),
    ] {
        window.paint_quad(fill(
            Bounds::new(pt(0., y - 0.5), size(px(w), px(1.))),
            color,
        ));
    }

    let line = curve(&knobs, w);
    let tint = if on { c.signal } else { c.text_faint };
    let mut area = PathBuilder::fill();
    area.move_to(pt(0., zero));
    for &(x, y) in &line {
        area.line_to(pt(x, y));
    }
    area.line_to(pt(w, zero));
    area.close();
    if let Ok(path) = area.build() {
        window.paint_path(path, tint.opacity(if on { 0.16 } else { 0.1 }));
    }
    let mut stroke = PathBuilder::stroke(px(2.));
    if let Some(&(x, y)) = line.first() {
        stroke.move_to(pt(x, y));
    }
    for &(x, y) in line.iter().skip(1) {
        stroke.line_to(pt(x, y));
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, tint);
    }

    for (i, &(x, y)) in knobs.iter().enumerate() {
        let d = if active == Some(i) { KNOB_ACTIVE } else { KNOB };
        window.paint_quad(quad(
            Bounds::new(pt(x - d / 2., y - d / 2.), size(px(d), px(d))),
            px(d / 2.),
            if on { c.text } else { c.text_muted },
            px(3.),
            c.overlay,
            BorderStyle::Solid,
        ));
    }
}

/// A Catmull-Rom curve through the knobs, flat out to the field's edges.
fn curve(knobs: &[(f32, f32)], w: f32) -> Vec<(f32, f32)> {
    let mut points = Vec::with_capacity(knobs.len() + 2);
    let first = knobs.first().map_or(0., |k| k.1);
    let last = knobs.last().map_or(0., |k| k.1);
    points.push((0., first));
    points.extend_from_slice(knobs);
    points.push((w, last));
    let n = points.len() as isize;
    let get = |i: isize| points[i.clamp(0, n - 1) as usize];
    const STEPS: usize = 10;
    let mut out = Vec::with_capacity(points.len() * STEPS);
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (get(i - 1), get(i), get(i + 1), get(i + 2));
        for k in 0..STEPS {
            let t = k as f32 / STEPS as f32;
            let cr = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2. * b
                    + (c - a) * t
                    + (2. * a - 5. * b + 4. * c - d) * t * t
                    + (3. * b - a - 3. * c + d) * t * t * t)
            };
            out.push((cr(p0.0, p1.0, p2.0, p3.0), cr(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    out.push(points[points.len() - 1]);
    out
}

/// "31", "1k": the column labels; the unit is implied.
fn short_label(band: usize) -> String {
    match BANDS.get(band) {
        Some(&f) if f >= 1000 => format!("{}k", f / 1000),
        Some(f) => f.to_string(),
        None => String::new(),
    }
}
