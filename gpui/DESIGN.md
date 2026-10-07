# ytfast GPUI design system

The look of the GPUI app, and the reference for every view built on it.
Tokens live in `src/theme.rs`; shared recipes in `src/views/widgets.rs`.
Views never name a colour, size or font directly: they use these.

## Principles

1. **YouTube Music's map, our own finish.** Sidebar, shelves of square covers,
   Quick picks rows, a bottom player bar, YouTube Music's labels. People should
   know where everything is.
2. **Cover art is the colour.** Chrome is near-neutral ink with a faint violet
   cast. The only chromatic UI colour is *signal*, and it means one thing:
   **live** (the playing song, playback progress, a toggle that is on).
   Content colours from YouTube (mood dots) are allowed because they are content.
3. **Depth from surfaces, not lines.** The window's `base` frames a rounded,
   brighter `surface` panel holding the page. Raised fills, translucent
   interaction overlays and soft shadows separate things; hairlines are a last
   resort.
4. **One type family, clear steps.** Inter, with Inter Display for big titles.
   Weight and size carry the hierarchy, never colour alone.
5. **Quiet motion.** Hover and press change at once. Things that appear settle
   in (opacity, `motion::BASE`, ease out). Nothing loops except loading.
   How much moves, and how fast, is the listener's call (Settings → Motion).

## Layout

```
┌ base ────────────────────────────────────────────────────────────┐
│ ▶ Music        ┌ surface panel, radius LG ─────────────────────┐ │
│ ⌂ Home  ◀ fill │ ‹ ›  ( ⌕ Search …… )            ( ◯ account ) │ │
│ ◎ Explore      │ Shelf title                                   │ │
│ ▥ Library      │ ▢ ▢ ▢ ▢ ▢   cards scroll sideways             │ │
│                │ Quick picks: 4-row columns scroll sideways    │ │
│                └───────────────────────────────────────────────┘ │
│ ▣ Title/Artist        ⤨ ⏮ (▶) ⏭ ⟳                   🔊 ───●     │
│                  0:15 ━━━━━━●──────── 3:57                       │
└──────────────────────────────────────────────────────────────────┘
```

- Sidebar `size::SIDEBAR` (232) on `base`, items inset `space::MD`.
- Panel: `mt SM`, `mr SM`, `mb XS`, `radius::LG`, `overflow_hidden`.
- Top bar `size::TOP_BAR` (64), inside the panel; the brand row has the same
  height so both share a centre line.
- Page content: side gutter `size::GUTTER` (32), shelves `XXL + SM` (40)
  apart, a shelf's title `LG` (16) above its body. Content is left aligned.
- Player bar `size::PLAYER_BAR` (88) on `base`; song and volume columns are the
  same width (300) so the transport is centred on the window.

## Light and dark

The look follows the desktop live: `theme::init` reads the XDG desktop
portal's `org.freedesktop.appearance` `color-scheme` (KDE sets it from the
colour scheme) and listens for `SettingChanged`, switching the tokens and
gpui-component's theme and redrawing every window (`src/theme/portal.rs`).
No preference or no portal means dark. `YTFAST_GPUI_THEME=light|dark` pins
a look over the desktop.

## Colour tokens (`theme::colors(cx)` → `Colors`)

OKLCH is the source of truth; hex is the sRGB fallback. Ink hue 285, signal
hue 15.

| Token | Role | Dark | Light |
| --- | --- | --- | --- |
| `base` | window, sidebar, player bar | `oklch(0.145 0.006 285)` #0a0a0d | `oklch(0.952 0.005 285)` #efeff2 |
| `surface` | page panel | `oklch(0.195 0.008 285)` #141418 | `oklch(0.995 0.002 285)` #fdfdff |
| `raised` | fields, chips, secondary buttons, placeholders | `oklch(0.245 0.009 285)` #202025 | `oklch(0.935 0.006 285)` #e9e9ed |
| `overlay` | menus, popovers, dialogs; raised hover | `oklch(0.27 0.01 285)` #26262b | `oklch(1 0 0)` #ffffff |
| `text` | titles, labels | `oklch(0.965 0.004 285)` #f3f3f6 | `oklch(0.21 0.012 285)` #18181e |
| `text_muted` | subtitles, artists (7.6:1 / 6.8:1) | `oklch(0.73 0.012 285)` #a7a7af | `oklch(0.47 0.014 285)` #5a5a63 |
| `text_faint` | numbers, times, disabled (≥ 4.5:1 on surface) | `oklch(0.6 0.012 285)` #7f7f87 | `oklch(0.56 0.012 285)` #73747b |
| `signal` | live: playing, progress, toggles on | `oklch(0.68 0.2 15)` #fa5570 | `oklch(0.57 0.21 15)` #d6224e |
| `danger` | errors | `oklch(0.7 0.17 35)` #f57050 | `oklch(0.55 0.19 35)` #c83406 |
| `success` | confirmations | `oklch(0.74 0.15 155)` #4bc680 | `oklch(0.55 0.14 155)` #00884b |
| `primary` / `primary_foreground` | Play buttons | text / base | text / surface |
| `on_media` / `on_media_foreground` | glyphs and discs on cover art (both looks) | #fcfcfc / base | #fcfcfc / text |

Translucent tokens (same recipe in both looks, alpha on `text`):
`hover` 6% (light 5%), `pressed` 10% (9%), `selected` 9% (7%), `hairline` 8%
(9%), `focus_ring` 40% (35%). `outline` is pure white 8% on dark, pure black
8% on light (never tinted). `scrim` is black 40% (35%). `signal_soft` and
`danger_soft` are those colours at 16%/14% (12%/10%). `shadow` is black 50%
(14%).

gpui-component's `Theme` is filled from these in `theme::map_colors`, so
`Button`, `Input`, `Slider`, `Spinner`, `Skeleton`, lists and scrollbars match.
Prefer our recipes; when you use a kit component, don't recolour it per view.

## Type (`theme::Type`, on any `Styled`)

Inter 4.001 static cuts, bundled (`assets/fonts`, OFL). Body is 14 px.

| Style | Size/line | Weight | Font | Use |
| --- | --- | --- | --- | --- |
| `type_display()` | 32/38 | Bold | Inter Display | page header title (max 2 lines) |
| `type_title()` | 22/28 | Bold | Inter Display | shelf titles |
| `type_heading()` | 17/22 | SemiBold | Inter | brand, dialog titles, empty-state titles |
| `type_label()` | 14/20 | Medium | Inter | song/card titles, nav, buttons, chips |
| `type_body()` | 14/20 | Regular | Inter | running text, messages, search |
| `type_small()` | 13/18 | Regular | Inter | subtitles, artists, descriptions |
| `type_caption()` | 12/16 | Medium | Inter | straplines, player times |

- `.tabular()` on every number that changes or lines up: times, durations,
  track numbers, counts.
- One line, `.truncate()`, for titles and subtitles in rows and cards. Header
  titles `line_clamp(2)`, descriptions `line_clamp(2)` and `max_w(640)`.
- Copy in sentence case as YouTube gives it; never uppercase in code.

## Spacing, radii, sizes

- `space`: `XXS 2 · XS 4 · SM 8 · MD 12 · LG 16 · XL 24 · XXL 32 · XXXL 48`.
  Gaps inside a component SM–MD; between components LG–XL; between shelves 40.
- `radius`: `XS 4` row thumbs · `SM 6` player cover · `MD 8` cards, rows, nav
  items, error strip · `LG 12` panel, header cover, dialogs · `FULL` pills,
  discs, artist covers. `widgets::cover` picks the radius from the size.
  Nested shapes are concentric: outer = inner + padding.
- `size`: card 176, header cover 224, row 56, row thumb 40, Quick picks column
  380, player cover 56, nav item 40, icon button 36, play disc 40, chip/pill
  36, icon 18 (small 16).

## Elevation and motion

- `elevation::low(c)`: play disc on a cover. `elevation::high(c)`: menus,
  popovers, dialogs (with `overlay` fill and `radius::LG`).
- `motion::FAST 120 ms` (menus, icon swaps, toggles), `BASE 200 ms`
  (content arriving, panels opening), `SLOW 320 ms` (sliding panels, a page
  sliding or scaling in, a lyric line growing, the cover flying); easing
  `motion::ease_out` (quint). Animate opacity and position only.
- Every animation goes through `motion::animate` / `.with_motion(id, kind,
  base, ..)`, never a bare `with_animation`: it scales `base` by the speed
  and draws the last frame at once when the kind is switched off or motion
  is reduced. Motion a view drives itself asks `motion::progress` (page
  transitions) or `motion::duration` (the carousel glide).
- Loading pulses with `widgets::skeleton` (1.6 s, opacity 0.55–1) unless the
  pulse is off.

### Settings → Motion (`motion.json` in the config directory)

- Page transitions: None, Fade (the default), Slide, Scale. Forward comes
  from the right and Back from the left (`Pages::transition`). GPUI can't
  transform an element, so Slide is a relative offset and Scale is a clip
  that opens around the page at full size while it drifts in; the page
  never reflows while it moves. A page that arrives after its skeleton
  fades in on its own.
- Speed: instant, 0.5× to 2× (a hand edit may go from 0.1× to 4×).
- Switches per kind (`motion::Kind`): menus and popovers, panels and
  dialogs, toasts, Now Playing and Stage opening, skeleton pulse, lyrics.
- Reduced motion: System (the portal's `reduced-motion`, or KDE's animation
  speed at Instant), Always or Never. The result also sets GPUI's
  `reduce_motion`, so the kit's spinners stand still; motion driven by hand
  (timers, M8 effects) checks `theme::reduced_motion(cx)`.
- Lyrics (`views::glide`, Now Playing and Stage): the current line grows
  (100–120%) and fills from left to right as it is sung; past lines shrink a
  little and dim, upcoming lines dim less, far lines fade further. The view
  eases the current line to the top third or the centre. Text size S/M/L/XL
  and left or centred lines. Brightness is the `text` token's opacity.
  `YTFAST_GPUI_FAKE_LYRICS=<file.lrc>` gives every song local timed lyrics
  for checks (`gpui/fixtures/lyrics.lrc`).

## Icons

Lucide via `gpui_kit::assets::IconName`; add any you use to `ExtraIcons` in
`src/assets.rs`. Outline icons at 18 px (16 in pills and fields), coloured
`text_muted` at rest and `text` when active. Transport and play buttons use the
filled glyphs in `assets/icons/fill` through `assets::Glyph` (play, pause,
skip back/forward). A play triangle sits 2 px right of centre (optical).

## Components (`views/widgets.rs` and where they're used)

- **Sidebar item** (`sidebar::item`): 40 tall, `px MD`, `gap MD`, `radius MD`,
  icon + `type_label`. Rest `text_muted`; hover `hover` fill and `text`;
  active `selected` fill and `text`. No accent bar.
- **Sidebar library** (`views/sidebar/`): under the nav, `XL` below it and
  scrolling on its own (kit scrollbar). Signed in: a full-width New playlist
  pill (`raised`, hover `overlay`, 18 px plus), then Liked music and the
  account's playlists, then "Recently played" (`type_caption` `text_muted`
  heading, `XL` above). Rows are `size::LIBRARY_ROW` 48 tall, `px SM`,
  `gap MD`, a 32 cover (`LIBRARY_THUMB`), title `type_label` and subtitle
  `type_small` `text_muted`, both one line; radius `XS + SM` (concentric
  with the cover). The open page's row has the `selected` fill; the
  collection playing now gets a 16 px `AudioLines` in `signal`. Signed out:
  Explore's shortcuts as sidebar items and a `type_small` `text_faint`
  "Sign in to see your library". Nothing is shown while the account is
  being checked.
- **Rail**: below `size::RAIL_BELOW` (1000 px window width) the sidebar is
  `SIDEBAR_RAIL` (72) wide: the brand tile, icons and covers centred, the
  New playlist pill a 36 round button, labels as tooltips, section headings
  a 24 px `hairline`.
- **Brand**: 28 px `signal` tile, `radius MD`, white play glyph; "Music" in
  `type_heading`.
- **Search field**: kit `Input` as a 40 px pill on `raised` with no visible
  border, leading search icon, `type_body`, clear button.
- **Shelf header**: optional strapline `type_caption` `text_muted`, title
  `type_title`, `XXS` between them, `LG` above the body.
- **Cover card** (`page::card`): 176 cover, title `type_label` `SM` below,
  subtitle `type_small` `text_muted`. Artists are round and centred. The
  card is a `group("card")`: on hover the cover gets `scrim` and a 40 px
  `on_media` play disc with `elevation::low` at the bottom-right (centre for
  round covers). The disc plays (`item.play`), the card opens.
- **Song row** (`page::row`): 56 tall, `px SM`, `gap MD`, `radius MD`; optional
  index (24 wide, right aligned, tabular, `text_faint`); 40 thumb; title
  `type_label`, subtitle `type_small` `text_muted`; duration `type_small`
  tabular `text_faint`. Hover `hover` fill and a play glyph on a `scrim` over
  the thumb; press `pressed`. Playing: `selected` fill, title and index in
  `signal`, `AudioLines` on the thumb's scrim.
- **Quick picks** (`RowCarousel`): columns of four rows, 380 wide, `LG` apart,
  scrolling sideways.
- **Chip** (`page::chip`): 36 pill on `raised`, `px LG`, `type_label`, hover
  `overlay`; YouTube's mood colour as an 8 px leading dot.
- **Buttons** (`widgets::pill_button`): 36 pill, optional 16 px leading icon.
  `Pill::Primary` (`primary` fill, the one main action, e.g. Play) and
  `Pill::Secondary` (`raised` fill: Shuffle, Radio, Try again, Dismiss).
  `widgets::icon_button`: 36 round ghost with `hover`/`pressed` fills; toggles
  colour their icon with `widgets::toggle_color` (`signal` when on).
- **Page header**: cover 224 (`radius LG`, round for artists), `XXL` gap, text
  column bottom-aligned: `type_display`, subtitle `type_body` muted, second
  subtitle and description `type_small` muted, then a row of pills.
- **Player bar**: cover 56 + title `type_label` / artists `type_small` muted
  ("Nothing playing" in `text_faint` when empty). Transport: shuffle, previous,
  the 40 px `primary` play disc (spinner while loading), next, repeat.
  Volume: icon by level and a 112 px slider in `text_muted`.
- **Seek bar**: at most 600 wide between the elapsed time and the length
  (`type_caption().tabular()` in `text_faint`). The effects layer draws it
  (M9, `visuals::bar`, over the cover's palette glow): the played part is a
  `signal` fill with a soft bloom that keeps the track's top edge and hangs
  below it as deep as the song is loud (its waveform, lit along the top),
  the rest `text` at 16-20%, and a `text` playhead that swells on the kick
  with a `signal` ring pulsing out of it (larger under the pointer). The
  kit `Slider` stays on top, see-through, for clicks and drags; the
  most-replayed ridge rises from the same edge over it. Without effects
  (`YTFAST_GPUI_VISUALS=0`, no GPU, Stage) it is the plain kit slider in
  `signal` with a `text` thumb.
- **Panels** (Up next, settings, dialogs): `overlay` (dialogs) or `surface`
  (side panel, `radius LG`, inset like the page panel), `elevation::high` when
  floating, padding `LG`–`XL`, headings `type_heading`.
- **Covers** (`widgets::cover`): always use it. It draws the placeholder (a
  music note on `raised`) under the image so loading, failed and missing covers
  look deliberate, and a 1 px `outline` inside the edge.
- **Empty, loading, error**: loading shows the page's shape with
  `widgets::skeleton` (title bar + cards), never a lone spinner on a page.
  Error/empty: centred 48 px `raised` disc with a muted icon, a `type_heading`
  line saying what happened, a `type_small` muted detail, and one
  secondary pill with the fix ("Try again"). The error strip under the top bar:
  `danger_soft` fill, `radius MD`, alert icon in `danger`, `type_small` text,
  Dismiss.

## Do and don't

- Do read colours from `theme::colors(cx)`; don't use `rgb()`/`hsla()` in views
  (content colours from the model are the only exception).
- Do use `signal` only for live state; don't use it for links, focus, selection
  of nav items or decoration.
- Do let covers carry the colour; don't tint surfaces per page.
- Do use `space`/`radius`/`size` constants; don't add one-off pixel values
  unless a recipe calls for it (then add a constant).
- Do keep hover states instant and press feedback a fill change; don't animate
  hover or add bouncing.
- Do put titles in one line with ellipsis; don't let rows grow taller.
- Don't put a `hover()` on an element twice (GPUI asserts in debug builds).
- Don't call `.hover()` after `widgets::icon_button`/`pill_button`; they have
  one already. Build a variant in `widgets` instead.
- Test both looks: `YTFAST_GPUI_THEME=light` and `=dark`, or switch the
  desktop (`plasma-apply-colorscheme BreezeDark`) while the app runs.
