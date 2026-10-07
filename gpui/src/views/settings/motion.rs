//! Settings → Motion and Lyrics: how pages arrive, how fast everything
//! moves, what moves at all, reduced motion, and how timed lyrics glide.
//! Every change saves to `motion.json` and shows at once
//! (`theme::motion`).

use gpui_kit::component::h_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::widgets;
use crate::app::MusicApp;
use crate::theme::motion::{self, Align, Anchor, Config, Lyrics, PageStyle, Reduce, TextSize};
use crate::theme::{Colors, Type, radius, space};

/// The speeds offered (0 is instant).
const SPEEDS: [(f32, &str); 6] = [
    (0.0, "Instant"),
    (0.5, "0.5×"),
    (0.75, "0.75×"),
    (1.0, "1×"),
    (1.5, "1.5×"),
    (2.0, "2×"),
];
/// The current line's size against the others.
const SCALES: [(f32, &str); 4] = [(1.0, "100%"), (1.05, "105%"), (1.1, "110%"), (1.2, "120%")];
/// How far other lines fade.
const DIMS: [(f32, &str); 4] = [
    (0.0, "None"),
    (0.3, "Low"),
    (0.55, "Medium"),
    (0.75, "High"),
];
const SEGMENT_H: Pixels = px(28.);

/// A switch: its id, its label and line, whether it is on, and what it
/// sets.
type Switch = (
    &'static str,
    &'static str,
    &'static str,
    bool,
    fn(&mut Config, bool),
);

/// Tab 0 is Motion, tab 1 Lyrics.
pub fn page(tab: usize, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let m = motion::config();
    if tab == 0 {
        let mut choices = motion_rows(&m, c, cx);
        let switches = choices.split_off(3);
        vec![
            super::section("", c, choices),
            super::section("What moves", c, switches),
        ]
    } else {
        vec![super::section("", c, lyrics_rows(&m, c, cx))]
    }
}

/// The tab's settings differ from the defaults.
pub fn changed(tab: usize) -> bool {
    let m = motion::config();
    if tab == 0 {
        m != Config {
            lyrics: m.lyrics,
            ..Config::default()
        }
    } else {
        m.lyrics != Lyrics::default()
    }
}

/// The tab's settings as Music starts out, the other tab's as they are.
pub fn reset(tab: usize, cx: &mut Context<MusicApp>) {
    motion::update(cx, |m| {
        if tab == 0 {
            *m = Config {
                lyrics: m.lyrics,
                ..Config::default()
            };
        } else {
            m.lyrics = Lyrics::default();
        }
    });
}

fn motion_rows(m: &Config, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let pages = segmented(
        "motion-pages",
        PageStyle::ALL.map(|s| (s.label(), s == m.pages)),
        |m, i| m.pages = PageStyle::ALL[i],
        c,
        cx,
    );
    let speed = segmented(
        "motion-speed",
        SPEEDS.map(|(s, label)| (label, (s - m.speed).abs() < 0.01)),
        |m, i| m.speed = SPEEDS[i].0,
        c,
        cx,
    );
    let reduce = segmented(
        "motion-reduce",
        [
            ("System", m.reduce == Reduce::System),
            ("Always", m.reduce == Reduce::Always),
            ("Never", m.reduce == Reduce::Never),
        ],
        |m, i| m.reduce = [Reduce::System, Reduce::Always, Reduce::Never][i],
        c,
        cx,
    );
    let mut rows = vec![
        super::row(
            "Page transitions",
            Some("Forward from the right, Back from the left".into()),
            pages,
            c,
        ),
        super::row("Speed", Some(speed_detail(m.speed)), speed, c),
        super::row("Reduce motion", Some(reduce_detail(m).into()), reduce, c),
    ];
    let switches: [Switch; 5] = [
        (
            "motion-menus",
            "Menus and popovers",
            "Right-click menus, the sleep timer and the account menu",
            m.menus,
            |m, on| m.menus = on,
        ),
        (
            "motion-panels",
            "Panels and dialogs",
            "Settings, Up next, the shortcuts and Play anything",
            m.panels,
            |m, on| m.panels = on,
        ),
        (
            "motion-toasts",
            "Notices above the player",
            "Short notes such as Link copied",
            m.toasts,
            |m, on| m.toasts = on,
        ),
        (
            "motion-now-playing",
            "Opening Now Playing and Stage",
            "The cover and the view growing into place",
            m.now_playing,
            |m, on| m.now_playing = on,
        ),
        (
            "motion-skeleton",
            "Loading placeholders pulse",
            "The shapes shown while a page loads",
            m.skeleton,
            |m, on| m.skeleton = on,
        ),
    ];
    rows.extend(
        switches
            .into_iter()
            .map(|(id, label, detail, on, set)| toggle(id, label, Some(detail), on, set, c, cx)),
    );
    rows
}

fn lyrics_rows(m: &Config, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let l = m.lyrics;
    let scale = segmented(
        "lyrics-scale",
        SCALES.map(|(s, label)| (label, (s - l.scale).abs() < 0.01)),
        |m, i| m.lyrics.scale = SCALES[i].0,
        c,
        cx,
    );
    let dim = segmented(
        "lyrics-dim",
        DIMS.map(|(d, label)| (label, (d - l.dim).abs() < 0.01)),
        |m, i| m.lyrics.dim = DIMS[i].0,
        c,
        cx,
    );
    let anchor = segmented(
        "lyrics-anchor",
        [
            ("Top third", l.anchor == Anchor::Third),
            ("Centre", l.anchor == Anchor::Centre),
        ],
        |m, i| m.lyrics.anchor = [Anchor::Third, Anchor::Centre][i],
        c,
        cx,
    );
    let size = segmented(
        "lyrics-size",
        TextSize::ALL.map(|s| (s.label(), s == l.size)),
        |m, i| m.lyrics.size = TextSize::ALL[i],
        c,
        cx,
    );
    let align = segmented(
        "lyrics-align",
        [
            ("Left", l.align == Align::Left),
            ("Centre", l.align == Align::Centre),
        ],
        |m, i| m.lyrics.align = [Align::Left, Align::Centre][i],
        c,
        cx,
    );
    vec![
        toggle(
            "lyrics-glide",
            "Glide between lines",
            Some("Lines grow and fade, and the view scrolls smoothly"),
            l.glide,
            |m, on| m.lyrics.glide = on,
            c,
            cx,
        ),
        super::row(
            "Current line size",
            Some("How much bigger the line being sung is".into()),
            scale,
            c,
        ),
        super::row(
            "Dim other lines",
            Some("How far the lines around it fade".into()),
            dim,
            c,
        ),
        toggle(
            "lyrics-fade-far",
            "Fade far lines",
            Some("Lines further from the current one are fainter"),
            l.fade_far,
            |m, on| m.lyrics.fade_far = on,
            c,
            cx,
        ),
        toggle(
            "lyrics-sweep",
            "Fill the line as it's sung",
            Some("The current line lights up from left to right"),
            l.sweep,
            |m, on| m.lyrics.sweep = on,
            c,
            cx,
        ),
        super::row(
            "Current line position",
            Some("Where the view keeps the line being sung".into()),
            anchor,
            c,
        ),
        super::row(
            "Text size",
            Some("Lyrics in Now Playing and Stage".into()),
            size,
            c,
        ),
        super::row(
            "Alignment",
            Some("Lines start at the left or sit centred".into()),
            align,
            c,
        ),
    ]
}

fn speed_detail(speed: f32) -> SharedString {
    match motion::at_speed(motion::BASE, speed) {
        Some(d) => format!("Panels open in {} ms", d.as_millis()).into(),
        None => "Everything changes at once".into(),
    }
}

fn reduce_detail(m: &Config) -> &'static str {
    match (m.reduce, motion::desktop_reduced()) {
        (Reduce::System, true) => "As the desktop asks: reduced",
        (Reduce::System, false) => "As the desktop asks: full motion",
        (Reduce::Always, _) => "Nothing moves, whatever the desktop asks",
        (Reduce::Never, _) => "Motion stays on, whatever the desktop asks",
    }
}

/// A switch row; `set` changes the setting.
fn toggle(
    id: &'static str,
    label: &'static str,
    detail: Option<&'static str>,
    on: bool,
    set: fn(&mut Config, bool),
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let control = widgets::switch(id, on, c).on_click(cx.listener(move |_, on: &bool, _, cx| {
        let on = *on;
        motion::update(cx, |m| set(m, on));
    }));
    super::row(label, detail.map(SharedString::from), control, c)
}

/// Choices side by side in one rounded track, the chosen one filled as
/// the filter chips are; `set` applies choice `i`.
fn segmented<const N: usize>(
    id: &'static str,
    options: [(&'static str, bool); N],
    set: fn(&mut Config, usize),
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> impl IntoElement + use<N> {
    let segments = options.into_iter().enumerate().map(|(i, (label, chosen))| {
        // The track shows through the ones not chosen.
        let (bg, fg, hover) = if chosen {
            (Some(c.primary), c.primary_foreground, c.primary_hover)
        } else {
            (None, c.text_muted, c.hover)
        };
        let name: SharedString = format!("{id}:{label}").into();
        let segment = div()
            .id((id, i))
            .debug_selector(move || name.to_string())
            .h(SEGMENT_H)
            .px(space::MD)
            .flex()
            .items_center()
            .rounded(radius::FULL)
            .when_some(bg, |s, bg| s.bg(bg))
            .text_color(fg)
            .type_small()
            .font_weight(FontWeight::MEDIUM)
            .tabular()
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .active(|s| s.opacity(0.9))
            .child(label)
            .on_click(cx.listener(move |_, _, _, cx| motion::update(cx, |m| set(m, i))));
        super::focusable(segment, c)
    });
    h_flex()
        .flex_none()
        .p(space::XXS)
        .gap(space::XXS)
        .rounded(radius::FULL)
        .bg(c.hover)
        .children(segments)
}
