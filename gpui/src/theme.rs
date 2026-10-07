//! The design system's tokens (see `gpui/DESIGN.md`): colours for the dark
//! and light look, the type scale, spacing, radii, elevation and motion.
//!
//! Views read colours only from [`colors`] (or the `cx.theme()` fields this
//! module sets for gpui-component), sizes from the constants here, and text
//! styles from [`Type`]. Nothing else names a colour.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::{Colorize, Theme, ThemeMode, oklch};
use gpui_kit::*;

/// The interface font, bundled (see `assets/fonts`), with fallbacks for
/// scripts it doesn't cover.
pub const FONT: &str = "Inter";
/// Inter's display cut, for page and shelf titles (22 px and up).
pub const FONT_DISPLAY: &str = "Inter Display";

#[cfg(target_os = "linux")]
mod portal;
#[cfg(not(target_os = "linux"))]
#[path = "theme/no_portal.rs"]
mod portal;

/// Which palette the window uses. It follows the desktop's light or dark
/// preference live (see [`portal`]); `YTFAST_GPUI_THEME=light|dark` pins one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dark,
    Light,
}

impl Mode {
    /// The look pinned by `YTFAST_GPUI_THEME`, if any.
    pub fn from_env() -> Option<Self> {
        match std::env::var("YTFAST_GPUI_THEME").as_deref() {
            Ok("light") => Some(Mode::Light),
            Ok("dark") => Some(Mode::Dark),
            _ => None,
        }
    }
}

/// Semantic colours. Surfaces step up in lightness in the dark look and the
/// page panel is the brightest surface in the light one; `signal` is the one
/// chromatic colour and marks what is live (see DESIGN.md).
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    /// The window behind everything: sidebar and player bar sit on it.
    pub base: Hsla,
    /// The page panel.
    pub surface: Hsla,
    /// Fills on the surface: search field, chips, cover placeholders.
    pub raised: Hsla,
    /// Menus, popovers, dialogs.
    pub overlay: Hsla,
    pub text: Hsla,
    /// Subtitles, artists, secondary labels.
    pub text_muted: Hsla,
    /// Track numbers, times, disabled and placeholder text.
    pub text_faint: Hsla,
    /// Translucent fills over any surface for interaction states.
    pub hover: Hsla,
    pub pressed: Hsla,
    pub selected: Hsla,
    /// 1 px separators, where spacing alone doesn't separate.
    pub hairline: Hsla,
    /// The 1 px outline inside covers.
    pub outline: Hsla,
    /// Darkens a cover under a hover overlay.
    pub scrim: Hsla,
    /// Glyphs and fills drawn on top of cover art (both looks).
    pub on_media: Hsla,
    pub on_media_foreground: Hsla,
    /// Primary buttons: the play buttons.
    pub primary: Hsla,
    pub primary_hover: Hsla,
    pub primary_foreground: Hsla,
    /// What is live: the playing song, playback progress, switched-on toggles.
    pub signal: Hsla,
    /// A tint of signal behind live things (a playing card's badge).
    #[allow(dead_code, reason = "a design-system token for views still to come")]
    pub signal_soft: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
    pub success: Hsla,
    pub focus_ring: Hsla,
    pub shadow: Hsla,
}

/// The surfaces' hue: a faint violet so the greys aren't flat.
const INK: f32 = 285.;
/// Signal's hue: a rose red, YouTube Music's red moved toward pink.
const SIGNAL: f32 = 15.;

impl Colors {
    pub fn dark() -> Self {
        let text = oklch(0.965, 0.004, INK);
        let signal = oklch(0.68, 0.2, SIGNAL);
        let danger = oklch(0.7, 0.17, 35.);
        let primary = text;
        Self {
            base: oklch(0.145, 0.006, INK),
            surface: oklch(0.195, 0.008, INK),
            raised: oklch(0.245, 0.009, INK),
            overlay: oklch(0.27, 0.01, INK),
            text,
            text_muted: oklch(0.73, 0.012, INK),
            text_faint: oklch(0.6, 0.012, INK),
            hover: text.opacity(0.06),
            pressed: text.opacity(0.1),
            selected: text.opacity(0.09),
            hairline: text.opacity(0.08),
            outline: white().opacity(0.08),
            scrim: black().opacity(0.4),
            on_media: oklch(0.99, 0., 0.),
            on_media_foreground: oklch(0.145, 0.006, INK),
            primary,
            primary_hover: primary.mix_oklab(oklch(0.145, 0.006, INK), 0.88),
            primary_foreground: oklch(0.145, 0.006, INK),
            signal,
            signal_soft: signal.opacity(0.16),
            danger,
            danger_soft: danger.opacity(0.14),
            success: oklch(0.74, 0.15, 155.),
            focus_ring: text.opacity(0.4),
            shadow: black().opacity(0.5),
        }
    }

    pub fn light() -> Self {
        let text = oklch(0.21, 0.012, INK);
        let signal = oklch(0.57, 0.21, SIGNAL);
        let danger = oklch(0.55, 0.19, 35.);
        Self {
            base: oklch(0.952, 0.005, INK),
            surface: oklch(0.995, 0.002, INK),
            raised: oklch(0.935, 0.006, INK),
            overlay: oklch(1., 0., 0.),
            text,
            text_muted: oklch(0.47, 0.014, INK),
            text_faint: oklch(0.56, 0.012, INK),
            hover: text.opacity(0.05),
            pressed: text.opacity(0.09),
            selected: text.opacity(0.07),
            hairline: text.opacity(0.09),
            outline: black().opacity(0.08),
            scrim: black().opacity(0.35),
            on_media: oklch(0.99, 0., 0.),
            on_media_foreground: oklch(0.21, 0.012, INK),
            primary: text,
            primary_hover: text.mix_oklab(oklch(0.995, 0.002, INK), 0.85),
            primary_foreground: oklch(0.995, 0.002, INK),
            signal,
            signal_soft: signal.opacity(0.12),
            danger,
            danger_soft: danger.opacity(0.1),
            success: oklch(0.55, 0.14, 155.),
            focus_ring: text.opacity(0.35),
            shadow: black().opacity(0.14),
        }
    }

    pub fn for_mode(mode: Mode) -> Self {
        match mode {
            Mode::Dark => Self::dark(),
            Mode::Light => Self::light(),
        }
    }
}

/// The active look, kept as a global next to gpui-component's `Theme`.
struct Look {
    mode: Mode,
    colors: Colors,
    /// `YTFAST_GPUI_THEME`: wins over the desktop.
    pinned: Option<Mode>,
}

impl Global for Look {}

/// The active colours.
pub fn colors(cx: &App) -> Colors {
    cx.global::<Look>().colors
}

/// The active mode.
#[allow(dead_code, reason = "a design-system token for views still to come")]
pub fn mode(cx: &App) -> Mode {
    cx.global::<Look>().mode
}

/// Whether the desktop asks for less motion. GPUI's animations
/// (`with_animation`, gpui-component's spinners) already settle at once
/// then; motion driven by hand (timers, M8 effects) checks this.
pub fn reduced_motion(cx: &App) -> bool {
    cx.reduce_motion()
}

/// Spacing scale (4 px grid). Use these, not one-off values.
pub mod space {
    use gpui_kit::{Pixels, px};
    pub const XXS: Pixels = px(2.);
    pub const XS: Pixels = px(4.);
    pub const SM: Pixels = px(8.);
    pub const MD: Pixels = px(12.);
    pub const LG: Pixels = px(16.);
    pub const XL: Pixels = px(24.);
    pub const XXL: Pixels = px(32.);
    pub const XXXL: Pixels = px(48.);
}

/// Corner radii. Nested shapes are concentric: outer = inner + padding.
pub mod radius {
    use gpui_kit::{Pixels, px};
    /// Row thumbnails, small badges.
    pub const XS: Pixels = px(4.);
    /// Player bar cover, small controls.
    pub const SM: Pixels = px(6.);
    /// Cards' covers, rows, nav items.
    pub const MD: Pixels = px(8.);
    /// Page header cover, the page panel, dialogs.
    pub const LG: Pixels = px(12.);
    /// Pills and circles.
    pub const FULL: Pixels = px(9999.);
}

/// Fixed sizes shared across views.
pub mod size {
    use gpui_kit::{Pixels, px};
    pub const SIDEBAR: Pixels = px(232.);
    /// The sidebar as a rail of icons and covers, in narrow windows.
    pub const SIDEBAR_RAIL: Pixels = px(72.);
    /// Windows narrower than this get the rail.
    pub const RAIL_BELOW: Pixels = px(1000.);
    /// A playlist or recently played row in the sidebar, and its cover.
    pub const LIBRARY_ROW: Pixels = px(48.);
    pub const LIBRARY_THUMB: Pixels = px(32.);
    pub const TOP_BAR: Pixels = px(64.);
    pub const PLAYER_BAR: Pixels = px(88.);
    /// The page's side gutter.
    pub const GUTTER: Pixels = px(32.);
    pub const CARD: Pixels = px(176.);
    pub const HEADER_COVER: Pixels = px(224.);
    pub const ROW: Pixels = px(56.);
    pub const ROW_THUMB: Pixels = px(40.);
    /// A Quick picks column.
    pub const ROW_COLUMN: Pixels = px(380.);
    pub const PLAYER_COVER: Pixels = px(56.);
    pub const NAV_ITEM: Pixels = px(40.);
    /// Icon buttons: 36 px target, 40 px for the play button.
    pub const ICON_BUTTON: Pixels = px(36.);
    pub const PLAY_BUTTON: Pixels = px(40.);
    pub const CHIP: Pixels = px(36.);
    pub const ICON: Pixels = px(18.);
    pub const ICON_SM: Pixels = px(16.);
}

/// Motion: durations and the one easing curve. Hover and press states
/// change at once; motion is for things that appear or move.
pub mod motion {
    use super::Duration;
    /// Small state changes: icons swapping, a toggle.
    #[allow(dead_code, reason = "a design-system token for views still to come")]
    pub const FAST: Duration = Duration::from_millis(120);
    /// Page content arriving, panels opening.
    pub const BASE: Duration = Duration::from_millis(200);
    /// Larger moves: a panel sliding, the cover flying.
    #[allow(dead_code, reason = "a design-system token for views still to come")]
    pub const SLOW: Duration = Duration::from_millis(320);
    /// Ease out (quint): starts fast, settles softly.
    pub fn ease_out(t: f32) -> f32 {
        1.0 - (1.0 - t).powi(5)
    }
}

/// Elevation: layered translucent shadows instead of borders.
pub mod elevation {
    use super::*;

    /// Things floating just above a surface: the play button on a cover.
    pub fn low(c: &Colors) -> Vec<BoxShadow> {
        vec![
            shadow(c.shadow.opacity(0.6), 1., 2.),
            shadow(c.shadow.opacity(0.4), 4., 12.),
        ]
    }

    /// Menus, popovers, dialogs.
    #[allow(dead_code, reason = "a design-system token for views still to come")]
    pub fn high(c: &Colors) -> Vec<BoxShadow> {
        vec![
            shadow(c.shadow.opacity(0.5), 2., 6.),
            shadow(c.shadow, 16., 40.),
        ]
    }

    fn shadow(color: Hsla, y: f32, blur: f32) -> BoxShadow {
        BoxShadow {
            color,
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.),
            inset: false,
        }
    }
}

/// The type scale as text styles. One family (Inter) in two optical cuts;
/// weight and size carry the hierarchy.
pub trait Type: Styled + Sized {
    /// Page titles: 32/38 Inter Display bold.
    fn type_display(self) -> Self {
        self.font_family(FONT_DISPLAY)
            .text_size(px(32.))
            .line_height(px(38.))
            .font_weight(FontWeight::BOLD)
    }
    /// Shelf titles: 22/28 Inter Display bold.
    fn type_title(self) -> Self {
        self.font_family(FONT_DISPLAY)
            .text_size(px(22.))
            .line_height(px(28.))
            .font_weight(FontWeight::BOLD)
    }
    /// The brand, dialog titles: 17/22 semibold.
    fn type_heading(self) -> Self {
        self.font_family(FONT)
            .text_size(px(17.))
            .line_height(px(22.))
            .font_weight(FontWeight::SEMIBOLD)
    }
    /// Running text: 14/20 regular.
    fn type_body(self) -> Self {
        self.font_family(FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .font_weight(FontWeight::NORMAL)
    }
    /// Song and card titles, nav labels, buttons: 14/20 medium.
    fn type_label(self) -> Self {
        self.font_family(FONT)
            .text_size(px(14.))
            .line_height(px(20.))
            .font_weight(FontWeight::MEDIUM)
    }
    /// Subtitles and secondary lines: 13/18 regular.
    fn type_small(self) -> Self {
        self.font_family(FONT)
            .text_size(px(13.))
            .line_height(px(18.))
            .font_weight(FontWeight::NORMAL)
    }
    /// Straplines, times, counts: 12/16 medium.
    fn type_caption(self) -> Self {
        self.font_family(FONT)
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
    }
    /// Digits of equal width, for times and counters that change.
    fn tabular(self) -> Self {
        self.font_features(FontFeatures(Arc::new(vec![("tnum".into(), 1)])))
    }
}

impl<T: Styled + Sized> Type for T {}

/// Loads the bundled fonts, applies the desktop's look (or the pinned one)
/// and keeps following the desktop while the app runs.
pub fn init(cx: &mut App) {
    setup(portal::Portal::connect(), cx);
}

/// The look without the desktop's portal (D-Bus), for the UI tests.
#[cfg(test)]
pub fn init_without_desktop(cx: &mut App) {
    setup(None, cx);
}

fn setup(portal: Option<portal::Portal>, cx: &mut App) {
    load_fonts(cx);
    let pinned = Mode::from_env();
    let desktop = portal.as_ref().map(|p| p.desktop).unwrap_or_default();
    let mode = pinned.or(desktop.scheme).unwrap_or(Mode::Dark);
    cx.set_global(Look {
        mode,
        colors: Colors::for_mode(mode),
        pinned,
    });
    apply(mode, cx);
    follow(desktop, cx);
    if let Some(portal) = portal {
        watch(portal, cx);
    }
}

/// Listens to the portal on a background task and applies each change on
/// the foreground.
fn watch(portal: portal::Portal, cx: &mut App) {
    let (tx, rx) = smol::channel::unbounded();
    cx.background_spawn(async move {
        if let Err(e) = portal.watch(tx).await {
            log::warn!("stopped following the desktop's appearance: {e:#}");
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        while let Ok(desktop) = rx.recv().await {
            cx.update(|cx| follow(desktop, cx));
        }
    })
    .detach();
}

/// Takes the desktop's light or dark (unless pinned) and its motion
/// preference, and redraws every window.
fn follow(desktop: portal::Desktop, cx: &mut App) {
    let look = cx.global::<Look>();
    let mode = look.pinned.or(desktop.scheme).unwrap_or(look.mode);
    log::info!(
        "desktop appearance: {:?} ({}), reduced motion {}",
        mode,
        match (look.pinned, desktop.scheme) {
            (Some(_), _) => "pinned by YTFAST_GPUI_THEME",
            (None, Some(_)) => "from the desktop",
            (None, None) => "no desktop preference",
        },
        desktop.reduced_motion()
    );
    if mode != look.mode {
        apply(mode, cx);
    }
    cx.set_reduce_motion(desktop.reduced_motion());
    cx.refresh_windows();
}

fn load_fonts(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = crate::assets::FONTS
        .iter()
        .map(|(_, bytes)| Cow::Borrowed(*bytes))
        .collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::warn!("loading the bundled fonts: {e}");
    }
}

/// Switches the look: our tokens, and gpui-component's theme from them so
/// its components (buttons, inputs, sliders, scrollbars) match.
pub fn apply(mode: Mode, cx: &mut App) {
    let colors = Colors::for_mode(mode);
    let look = cx.global_mut::<Look>();
    look.mode = mode;
    look.colors = colors;
    let theme_mode = match mode {
        Mode::Dark => ThemeMode::Dark,
        Mode::Light => ThemeMode::Light,
    };
    Theme::change(theme_mode, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = FONT.into();
        theme.radius = radius::MD;
        theme.radius_lg = radius::LG;
        theme.shadow = true;
        map_colors(&colors, theme);
    });
}

/// gpui-component's colour fields, filled from our tokens.
fn map_colors(c: &Colors, t: &mut Theme) {
    let solid_hover = c.hover.blend_on(c.surface);
    let k = &mut t.colors;
    k.background = c.surface;
    k.foreground = c.text;
    k.border = c.hairline;
    k.input = c.hairline;
    k.ring = c.focus_ring;
    k.caret = c.text;
    k.selection = c.signal.opacity(0.35);
    k.muted = c.raised;
    k.muted_foreground = c.text_muted;
    k.accent = solid_hover;
    k.accent_foreground = c.text;
    k.popover = c.overlay;
    k.popover_foreground = c.text;
    k.overlay = c.scrim;
    k.link = c.text;
    k.link_hover = c.text_muted;
    k.link_active = c.text_muted;

    k.primary = c.primary;
    k.primary_hover = c.primary_hover;
    k.primary_active = c.primary_hover;
    k.primary_foreground = c.primary_foreground;
    k.button_primary = c.primary;
    k.button_primary_hover = c.primary_hover;
    k.button_primary_active = c.primary_hover;
    k.button_primary_foreground = c.primary_foreground;

    k.secondary = c.raised;
    k.secondary_hover = c.overlay;
    k.secondary_active = c.overlay;
    k.secondary_foreground = c.text;
    k.button = c.raised;
    k.button_hover = c.overlay;
    k.button_active = c.overlay;
    k.button_foreground = c.text;
    k.button_secondary = c.raised;
    k.button_secondary_hover = c.overlay;
    k.button_secondary_active = c.overlay;
    k.button_secondary_foreground = c.text;

    k.danger = c.danger;
    k.danger_hover = c.danger;
    k.danger_active = c.danger;
    k.danger_foreground = c.primary_foreground;
    k.success = c.success;

    k.sidebar = c.base;
    k.sidebar_foreground = c.text;
    k.sidebar_accent = c.selected.blend_on(c.base);
    k.sidebar_accent_foreground = c.text;
    k.sidebar_border = c.base;
    k.sidebar_primary = c.primary;
    k.sidebar_primary_foreground = c.primary_foreground;

    k.list = c.surface;
    k.list_hover = solid_hover;
    k.list_active = c.selected.blend_on(c.surface);
    k.list_active_border = c.focus_ring;
    k.list_even = c.surface;
    k.list_head = c.surface;

    k.slider_bar = c.text_muted;
    k.slider_thumb = c.text;
    k.progress_bar = c.signal;
    k.switch = c.raised;
    k.switch_thumb = c.text;
    k.skeleton = c.raised;
    k.scrollbar = c.surface.opacity(0.);
    k.scrollbar_thumb = c.text.opacity(0.18);
    k.scrollbar_thumb_hover = c.text.opacity(0.32);
    k.title_bar = c.base;
    k.title_bar_border = c.base;
    k.window_border = c.hairline;
    k.tab_bar = c.base;
    k.tab = c.base;
    k.tab_active = c.raised;
    k.tab_foreground = c.text_muted;
    k.tab_active_foreground = c.text;
}

/// Flattens a translucent colour onto the surface below it, for fields that
/// gpui-component expects to be opaque.
trait BlendOn {
    fn blend_on(self, below: Hsla) -> Hsla;
}

impl BlendOn for Hsla {
    fn blend_on(self, below: Hsla) -> Hsla {
        let top = Hsla { a: 1., ..self };
        top.mix_oklab(below, self.a)
    }
}
