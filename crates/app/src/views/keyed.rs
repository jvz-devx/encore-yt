//! M29: controls that answer the keyboard, and the focus ring they show.
//!
//! - The ring: a 2 px `focus_ring` line while the keyboard (not the
//!   pointer) is on a control. On an opaque fill (a chosen chip, a primary
//!   button) it stands just outside; on anything else it sits inside the
//!   edge, since a shadow outside would show through a translucent or
//!   missing fill as a light patch.
//! - [`Chip`]s in a row ([`arrow_row`]): ←/→ choose the one before or
//!   after and take the keyboard along, as a radio group does.
//! - [`slider`]: ←/→ step, Shift+←/→ ten steps, Home/End the ends.

use std::rc::Rc;

use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::*;

use crate::theme::{Colors, radius, space};

actions!(
    keyed,
    [
        ChoicePrevious,
        ChoiceNext,
        SliderLess,
        SliderMore,
        SliderMuchLess,
        SliderMuchMore,
        SliderLeast,
        SliderMost
    ]
);

/// The key context of a chip in an arrow row.
const CHOICE: &str = "KeyedChoice";
/// The key context of a slider.
const SLIDER: &str = "KeyedSlider";

pub fn bind_keys(cx: &mut App) {
    let choice = Some(CHOICE);
    let slider = Some(SLIDER);
    cx.bind_keys([
        KeyBinding::new("left", ChoicePrevious, choice),
        KeyBinding::new("right", ChoiceNext, choice),
        KeyBinding::new("left", SliderLess, slider),
        KeyBinding::new("right", SliderMore, slider),
        KeyBinding::new("shift-left", SliderMuchLess, slider),
        KeyBinding::new("shift-right", SliderMuchMore, slider),
        KeyBinding::new("home", SliderLeast, slider),
        KeyBinding::new("end", SliderMost, slider),
    ]);
}

fn ring_line(color: Hsla, inset: bool) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(2.),
        inset,
    }]
}

/// A tab stop with the ring inside its edge (no fill, or a translucent one).
pub fn ring_inside<E: InteractiveElement + StatefulInteractiveElement + Styled>(
    el: E,
    c: &Colors,
) -> E {
    let ring = c.focus_ring;
    el.tab_index(0)
        .focus_visible(move |s| s.shadow(ring_line(ring, true)))
}

/// A tab stop on an opaque fill, with the ring just outside it.
pub fn ring_outside<E: InteractiveElement + StatefulInteractiveElement + Styled>(
    el: E,
    c: &Colors,
) -> E {
    let ring = c.focus_ring;
    el.tab_index(0)
        .focus_visible(move |s| s.shadow(ring_line(ring, false)))
}

type Click = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// One of a row of choices: a stateful element that remembers what a click
/// does, so the arrows can choose it too.
pub struct Chip {
    el: Stateful<Div>,
    click: Option<Click>,
}

impl Chip {
    pub fn new(el: Stateful<Div>) -> Self {
        Self { el, click: None }
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        let f: Click = Rc::new(f);
        let g = f.clone();
        self.el = self.el.on_click(move |e, w, cx| g(e, w, cx));
        self.click = Some(f);
        self
    }
}

impl Styled for Chip {
    fn style(&mut self) -> &mut StyleRefinement {
        self.el.style()
    }
}

impl IntoElement for Chip {
    type Element = Stateful<Div>;

    fn into_element(self) -> Self::Element {
        self.el
    }
}

/// The chips with ←/→: each chooses its neighbour (skipping ones that
/// can't be chosen) and moves the keyboard onto it; at the ends nothing
/// happens. The chips must be tab stops next to each other.
pub fn arrow_row(chips: impl IntoIterator<Item = Chip>) -> Vec<Stateful<Div>> {
    let chips: Vec<Chip> = chips.into_iter().collect();
    let clicks: Rc<Vec<Option<Click>>> = Rc::new(chips.iter().map(|c| c.click.clone()).collect());
    chips
        .into_iter()
        .enumerate()
        .map(|(i, chip)| {
            let (back, on) = (clicks.clone(), clicks.clone());
            chip.el
                .key_context(CHOICE)
                .on_action(move |_: &ChoicePrevious, window, cx| {
                    let found = (0..i).rev().find(|&j| back[j].is_some());
                    if let Some(j) = found {
                        choose(&back, j, i - j, false, window, cx);
                    }
                })
                .on_action(move |_: &ChoiceNext, window, cx| {
                    let found = (i + 1..on.len()).find(|&j| on[j].is_some());
                    if let Some(j) = found {
                        choose(&on, j, j - i, true, window, cx);
                    }
                })
        })
        .collect()
}

/// Chooses chip `j`, `steps` tab stops away, and moves the focus there.
fn choose(
    clicks: &[Option<Click>],
    j: usize,
    steps: usize,
    forward: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(click) = &clicks[j] {
        let event = ClickEvent::Keyboard(KeyboardClickEvent {
            button: KeyboardButton::Enter,
            bounds: Bounds::default(),
        });
        click(&event, window, cx);
    }
    for _ in 0..steps {
        if forward {
            window.focus_next(cx);
        } else {
            window.focus_prev(cx);
        }
    }
}

/// A slider's keys and ring around it, on a box that holds the slider: it
/// reaches a little past the slider on each side so the ring clears the
/// thumb. Disabled, it is no tab stop.
pub fn slider(
    name: &'static str,
    state: &Entity<SliderState>,
    enabled: bool,
    c: &Colors,
) -> Stateful<Div> {
    let el = div()
        .id(("keyed-slider", state.entity_id()))
        .debug_selector(move || format!("slider:{name}"))
        .mx(-space::SM)
        .px(space::SM)
        .my(-space::XS)
        .py(space::XS)
        .rounded(radius::FULL);
    if !enabled {
        return el;
    }
    let by = |state: &Entity<SliderState>, f: fn(&SliderState) -> f32| {
        let state = state.clone();
        move |window: &mut Window, cx: &mut App| set(&state, f, window, cx)
    };
    let less = by(state, |s| s.value().end() - step(s));
    let more = by(state, |s| s.value().end() + step(s));
    let much_less = by(state, |s| s.value().end() - step(s) * 10.);
    let much_more = by(state, |s| s.value().end() + step(s) * 10.);
    let least = by(state, SliderState::min_value);
    let most = by(state, SliderState::max_value);
    ring_inside(el, c)
        .key_context(SLIDER)
        .on_action(move |_: &SliderLess, w, cx| less(w, cx))
        .on_action(move |_: &SliderMore, w, cx| more(w, cx))
        .on_action(move |_: &SliderMuchLess, w, cx| much_less(w, cx))
        .on_action(move |_: &SliderMuchMore, w, cx| much_more(w, cx))
        .on_action(move |_: &SliderLeast, w, cx| least(w, cx))
        .on_action(move |_: &SliderMost, w, cx| most(w, cx))
}

/// An arrow's step: the slider's own, or a 200th of its range where that
/// is finer.
fn step(s: &SliderState) -> f32 {
    s.step_value().max((s.max_value() - s.min_value()) / 200.)
}

/// Moves a slider's (upper) value to `to`, kept in range and on its step,
/// and tells its listeners as a finished drag would.
fn set(
    state: &Entity<SliderState>,
    to: fn(&SliderState) -> f32,
    window: &mut Window,
    cx: &mut App,
) {
    state.update(cx, |s, cx| {
        let (min, max, step) = (s.min_value(), s.max_value(), s.step_value());
        let mut v = to(s).clamp(min, max);
        if step > 0. {
            v = (min + ((v - min) / step).round() * step).clamp(min, max);
        }
        let value = match s.value() {
            gpui_kit::component::slider::SliderValue::Range(start, _) => (start.min(v), v).into(),
            gpui_kit::component::slider::SliderValue::Single(_) => v.into(),
        };
        s.set_value(value, window, cx);
        cx.emit(SliderEvent::Change(value));
        cx.emit(SliderEvent::Release(value));
    });
}
