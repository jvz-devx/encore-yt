//! Settings → About: which Music this is (version, update channel), its
//! licence and the projects it builds on (the README's Credits, and the
//! third-party notices the installers carry), and where its settings,
//! cache and logs live, each with a button that opens the folder.

use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::assets::Glyph;
use crate::settings::Category;
use crate::theme::{Colors, Type, radius, size, space};
use crate::update;

const REPOSITORY: &str = "https://github.com/jvz-devx/ytfast-gpui";
/// What the installers put next to the app (`packaging`).
const NOTICES: &str = include_str!("../../../../../packaging/THIRD-PARTY.txt");

/// Who made what Music is built on: name, what it does here, licence, link.
const CREDITS: &[(&str, &str, &str, &str)] = &[
    (
        "ytfast",
        "Where Music started, by Tyler Mayberry; its backend still runs it",
        "MIT",
        "https://github.com/MayberryDT/ytfast",
    ),
    (
        "fastframe",
        "Logging, and the self-updater, by Carmine Paolino",
        "MIT",
        "https://github.com/crmne/fastframe",
    ),
    (
        "GPUI",
        "The interface toolkit, by Zed Industries",
        "Apache-2.0",
        "https://github.com/zed-industries/zed",
    ),
    (
        "gpui-kit",
        "Controls on GPUI (gpui-component), by Longbridge",
        "Apache-2.0",
        "https://github.com/longbridge/gpui-kit",
    ),
    (
        "Symphonia and libopus",
        "Decode the audio",
        "MPL-2.0, BSD",
        "https://github.com/pdeljanov/Symphonia",
    ),
    (
        "cpal and rubato",
        "Play and resample it",
        "Apache-2.0, MIT",
        "https://github.com/RustAudio/cpal",
    ),
    (
        "yt-dlp-ejs and rquickjs",
        "Solve YouTube's stream challenges, after what yt-dlp learned",
        "Unlicense, MIT",
        "https://github.com/yt-dlp/ejs",
    ),
    ("LRCLIB", "Timed lyrics", "Open data", "https://lrclib.net"),
    (
        "souvlaki",
        "Media controls on Windows and macOS",
        "MIT",
        "https://github.com/Sinono3/souvlaki",
    ),
    (
        "Inter and Lucide",
        "The typeface and the icons",
        "OFL, ISC",
        "https://rsms.me/inter/",
    ),
];

pub fn page(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    vec![
        super::section("", c, [hero(c), channel(app, c, cx)]),
        super::section("Where files live", c, folders(app, c, cx)),
        super::section("Licence and credits", c, credits(app, c, cx)),
    ]
}

/// The mark, the name and the version.
fn hero(c: &Colors) -> AnyElement {
    h_flex()
        .py(space::LG)
        .gap(space::LG)
        .child(
            h_flex()
                .size(px(56.))
                .flex_none()
                .justify_center()
                .rounded(radius::LG)
                .bg(c.signal)
                // Nudged right: a triangle's visual centre is left of its box's.
                .pl(px(3.))
                .child(widgets::glyph(Glyph::Play, px(26.), c.on_media)),
        )
        .child(
            v_flex()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_title().child("Music"))
                .child(
                    div()
                        .type_small()
                        .tabular()
                        .text_color(c.text_muted)
                        .child(format!("Version {}", update::VERSION)),
                )
                .child(widgets::muted_line(
                    "A native YouTube Music client: no browser engine, no telemetry",
                    c,
                )),
        )
        .into_any_element()
}

/// Which releases updates come from, with a way to Updates.
fn channel(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let prefs = &app.updates.prefs;
    let channel = if prefs.prereleases() {
        "Pre-releases and stable releases"
    } else {
        "Stable releases"
    };
    let checks = if prefs.check {
        "checked once a day"
    } else {
        "checks are off"
    };
    let button = super::focusable(
        widgets::pill_button("about-updates", "Updates", None, Pill::Tonal, c),
        c,
    )
    .on_click(cx.listener(|this, _, window, cx| this.show_category(Category::Updates, window, cx)));
    super::row(
        "Update channel",
        Some(format!("{channel}, {checks}").into()),
        button,
        c,
    )
}

/// Settings, cache and logs, each with Open.
fn folders(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let paths = &app.paths;
    let log = paths.cache.join("ytfast-gpui.log");
    vec![
        place("settings", "Settings", &paths.config, false, c, cx),
        place(
            "cache",
            "Cache: pages, covers and searches",
            &paths.cache,
            false,
            c,
            cx,
        ),
        place("logs", "Logs", &log, true, c, cx),
    ]
}

/// A place on disk and its Open (a file shows in its folder).
fn place(
    id: &'static str,
    label: &'static str,
    path: &Path,
    file: bool,
    c: &Colors,
    cx: &mut Context<MusicApp>,
) -> AnyElement {
    let target: PathBuf = path.to_path_buf();
    let button = super::focusable(
        widgets::pill_button(
            SharedString::from(format!("about-open-{id}")),
            "Open folder",
            Some(widgets::icon(IconName::FolderOpen, size::ICON_SM, c.text)),
            Pill::Tonal,
            c,
        ),
        c,
    )
    .on_click(cx.listener(move |_, _, _, cx| {
        if file {
            cx.reveal_path(&target);
        } else {
            cx.open_with_system(&target);
        }
    }));
    super::row(label, Some(home_relative(path).into()), button, c)
}

/// `path` with the home folder written `~`.
fn home_relative(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        },
        _ => path.display().to_string(),
    }
}

/// The licence, a line per project Music builds on, and the notices.
fn credits(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> Vec<AnyElement> {
    let source = super::focusable(
        widgets::pill_button(
            "about-source",
            "Source code",
            Some(widgets::icon(IconName::ExternalLink, size::ICON_SM, c.text)),
            Pill::Tonal,
            c,
        ),
        c,
    )
    .on_click(|_, _, cx| cx.open_url(REPOSITORY));
    let mut rows = vec![super::row(
        "MIT licence",
        Some("Free to use, change and share. Started from ytfast, also MIT.".into()),
        source,
        c,
    )];
    rows.extend(
        CREDITS
            .iter()
            .enumerate()
            .map(|(i, credit)| credit_row(i, *credit, c)),
    );
    rows.push(notices(app.settings.notices, c, cx));
    rows
}

/// A project: its name, what it does here and its licence; a click opens
/// its page.
fn credit_row(
    i: usize,
    (name, what, licence, url): (&'static str, &'static str, &'static str, &'static str),
    c: &Colors,
) -> AnyElement {
    let (hover, pressed) = (c.hover, c.pressed);
    let row = h_flex()
        .id(("about-credit", i))
        .mx(-space::SM)
        .px(space::SM)
        .py(space::SM)
        .gap(space::LG)
        .rounded(radius::SM)
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(space::XXS)
                .child(div().type_label().child(name))
                .child(widgets::muted_line(what, c)),
        )
        .child(
            div()
                .flex_none()
                .type_caption()
                .text_color(c.text_faint)
                .child(licence),
        )
        .child(widgets::icon(
            IconName::ExternalLink,
            size::ICON_SM,
            c.text_faint,
        ))
        .on_click(move |_, _, cx| cx.open_url(url));
    super::focusable(row, c).into_any_element()
}

/// The third-party notices, folded until asked for.
fn notices(open: bool, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let button = super::focusable(
        widgets::pill_button(
            "about-notices",
            if open { "Hide" } else { "Show" },
            None,
            Pill::Tonal,
            c,
        ),
        c,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        this.settings.notices = !this.settings.notices;
        cx.notify();
    }));
    v_flex()
        .child(super::row(
            "Third-party notices",
            Some("The licences the installers carry with Music".into()),
            button,
            c,
        ))
        .children(open.then(|| {
            div()
                .mb(space::MD)
                .p(space::LG)
                .rounded(radius::SM)
                .bg(c.raised)
                .type_small()
                .text_color(c.text_muted)
                .child(NOTICES)
        }))
        .into_any_element()
}
