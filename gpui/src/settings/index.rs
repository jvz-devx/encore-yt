//! What Settings' search looks through: every setting by its label and the
//! line explaining it, with the category (and tab) it lives in, plus every
//! keyboard shortcut. Labels match the rows in `views::settings`.

use super::Category;
use crate::desktop::{Group, SHORTCUTS, key_label};

/// One setting: where it is, its label and what it does.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub category: Category,
    /// The tab inside the category (`Category::tabs`).
    pub tab: Option<usize>,
    pub label: String,
    pub about: String,
}

/// Category, tab, label, line.
type Line = (Category, Option<usize>, &'static str, &'static str);

const ACCOUNT: &[Line] = &[
    (
        Category::Account,
        None,
        "Signed in",
        "Who YouTube Music says is signed in, and Reconnect",
    ),
    (
        Category::Account,
        None,
        "Use YouTube Music as",
        "Which of the account's channels to act as",
    ),
    (
        Category::Account,
        None,
        "Use the YouTube account signed in to",
        "The browser profile or cookie file the session comes from",
    ),
    (
        Category::Account,
        None,
        "Sign in another way",
        "Add a browser, a cookies file or pasted cookies",
    ),
];

const PLAYBACK: &[Line] = &[
    (
        Category::Playback,
        None,
        "Even out loudness between songs",
        "Loudness levelling: quiet and loud songs play at a similar volume",
    ),
    (
        Category::Playback,
        None,
        "Crossfade between songs",
        "Smooth mixes on radios, mixes and autoplay",
    ),
    (
        Category::Playback,
        None,
        "Crossfade length",
        "How long smooth mixes overlap two songs",
    ),
    (
        Category::Playback,
        None,
        "Sleep timer",
        "Fades out, then pauses playback",
    ),
    (
        Category::Playback,
        None,
        "Show a notification when the song changes",
        "Desktop notifications while Music's window isn't in front",
    ),
];

const EQUALIZER: &[Line] = &[
    (
        Category::Equalizer,
        None,
        "Use the equalizer",
        "Turn the equalizer on or off",
    ),
    (
        Category::Equalizer,
        None,
        "Presets",
        "Flat, bass boost, treble boost, vocal, rock, electronic, acoustic, late night",
    ),
    (
        Category::Equalizer,
        None,
        "Adjust bands",
        "Opens the equalizer to drag the ten bands",
    ),
];

const VISUALS: &[Line] = &[
    (
        Category::Visuals,
        Some(0),
        "Look",
        "Off, Calm, Default or Vivid: the preset for every effect",
    ),
    (
        Category::Visuals,
        Some(0),
        "Effects",
        "Backdrops, glows and the visualiser move with the music",
    ),
    (
        Category::Visuals,
        Some(0),
        "Frame rate",
        "How smoothly the effects move",
    ),
    (
        Category::Visuals,
        Some(1),
        "Backdrop",
        "The cover flowing behind Now Playing: colour, blur, swirl speed, bloom, bass pulse",
    ),
    (
        Category::Visuals,
        Some(1),
        "Backdrop in Stage",
        "The flowing cover behind Stage",
    ),
    (
        Category::Visuals,
        Some(2),
        "Particles",
        "Sparkles over the backdrop: amount, size, softness, brightness, speed, direction, depth, twinkle",
    ),
    (
        Category::Visuals,
        Some(2),
        "Wave",
        "Ribbons of the cover's colours: strength, speed, height",
    ),
    (
        Category::Visuals,
        Some(3),
        "Glow",
        "The cover's colours in the player bar",
    ),
    (
        Category::Visuals,
        Some(3),
        "Beat halos",
        "Rings pulsing from the seek bar on the beat",
    ),
    (
        Category::Visuals,
        Some(3),
        "Waveform",
        "Seek bar: the song's loudness under the played part",
    ),
    (
        Category::Visuals,
        Some(3),
        "Most replayed",
        "Seek bar: a ridge over the part people replay most",
    ),
    (
        Category::Visuals,
        Some(4),
        "Visualiser",
        "Style, bars, sensitivity, smoothing, fall speed, frequencies, colours",
    ),
    (
        Category::Visuals,
        Some(4),
        "Peak caps",
        "A cap over each bar that falls slowly",
    ),
    (
        Category::Visuals,
        Some(4),
        "Open visualiser",
        "The visualiser filling the window",
    ),
    (
        Category::Visuals,
        Some(5),
        "Cover dissolve",
        "The next cover dissolving in when the song changes",
    ),
    (
        Category::Visuals,
        Some(5),
        "Cover flight",
        "The cover flying into Now Playing",
    ),
];

const MOTION: &[Line] = &[
    (
        Category::Motion,
        Some(0),
        "Page transitions",
        "None, fade, slide or scale when you go somewhere",
    ),
    (
        Category::Motion,
        Some(0),
        "Speed",
        "How fast everything moves, or instant",
    ),
    (
        Category::Motion,
        Some(0),
        "Reduce motion",
        "Follow the desktop, always or never",
    ),
    (
        Category::Motion,
        Some(0),
        "Menus and popovers",
        "Animate menus and popovers",
    ),
    (
        Category::Motion,
        Some(0),
        "Panels and dialogs",
        "Animate panels and dialogs",
    ),
    (
        Category::Motion,
        Some(0),
        "Notices above the player",
        "Animate short notices",
    ),
    (
        Category::Motion,
        Some(0),
        "Opening Now Playing and Stage",
        "Animate Now Playing and Stage opening",
    ),
    (
        Category::Motion,
        Some(0),
        "Loading placeholders pulse",
        "Skeletons pulse while pages load",
    ),
    (
        Category::Motion,
        Some(1),
        "Glide between lines",
        "Lyrics grow and fade, and the view scrolls smoothly",
    ),
    (
        Category::Motion,
        Some(1),
        "Current line size",
        "How much bigger the lyric being sung is",
    ),
    (
        Category::Motion,
        Some(1),
        "Dim other lines",
        "How far the other lyrics fade",
    ),
    (
        Category::Motion,
        Some(1),
        "Fade far lines",
        "Lyrics further from the current one are fainter",
    ),
    (
        Category::Motion,
        Some(1),
        "Fill the line as it's sung",
        "The lyric fills from left to right",
    ),
    (
        Category::Motion,
        Some(1),
        "Current line position",
        "Lyrics: top third or centre",
    ),
    (
        Category::Motion,
        Some(1),
        "Text size",
        "Lyrics in small, medium, large or extra large",
    ),
    (
        Category::Motion,
        Some(1),
        "Alignment",
        "Lyrics left or centred",
    ),
];

const UPDATES: &[Line] = &[
    (
        Category::Updates,
        None,
        "Check now",
        "This version, and whether a newer one is out",
    ),
    (
        Category::Updates,
        None,
        "Check for updates",
        "Once a day, on GitHub Releases",
    ),
    (
        Category::Updates,
        None,
        "Include pre-releases",
        "Test versions before they're final",
    ),
];

const ABOUT: &[Line] = &[
    (
        Category::About,
        None,
        "Version",
        "Which version of Music this is, and its update channel",
    ),
    (
        Category::About,
        None,
        "Where files live",
        "Settings, cache and logs folders",
    ),
    (
        Category::About,
        None,
        "Licence and credits",
        "MIT licence, the projects Music builds on, third-party notices",
    ),
];

/// Every setting, in the sidebar's order.
pub fn entries() -> Vec<Entry> {
    let lines = [
        ACCOUNT, PLAYBACK, EQUALIZER, VISUALS, MOTION, UPDATES, ABOUT,
    ];
    let mut out: Vec<Entry> = lines
        .iter()
        .flat_map(|l| l.iter())
        .map(|(category, tab, label, about)| Entry {
            category: *category,
            tab: *tab,
            label: (*label).into(),
            about: (*about).into(),
        })
        .collect();
    out.extend(SHORTCUTS.iter().map(|s| {
        Entry {
            category: Category::Shortcuts,
            tab: Group::ALL.iter().position(|g| *g == s.group),
            label: s.what.into(),
            about: s
                .keys
                .iter()
                .map(|combo| {
                    combo
                        .iter()
                        .map(|k| key_label(k))
                        .collect::<Vec<_>>()
                        .join("+")
                })
                .collect::<Vec<_>>()
                .join(" or "),
        }
    }));
    out.sort_by_key(|e| Category::ALL.iter().position(|c| *c == e.category));
    out
}

/// The settings matching `query`: every word of it in the label or the
/// line, labels that start with it first within each category.
pub fn search(query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut hits: Vec<(usize, u8, Entry)> = entries()
        .into_iter()
        .filter_map(|e| {
            let label = e.label.to_lowercase();
            let text = format!("{label} {} {}", e.about.to_lowercase(), tab_name(&e));
            if !words.iter().all(|w| text.contains(w)) {
                return None;
            }
            let rank = if label.starts_with(&query) {
                0
            } else if label.contains(&query) {
                1
            } else {
                2
            };
            let order = Category::ALL.iter().position(|c| *c == e.category)?;
            Some((order, rank, e))
        })
        .collect();
    hits.sort_by_key(|(order, rank, _)| (*order, *rank));
    hits.into_iter().map(|(_, _, e)| e).collect()
}

/// The tab's name, lowercase, so "lyrics" finds the Lyrics tab's settings.
fn tab_name(e: &Entry) -> String {
    e.tab
        .and_then(|t| e.category.tabs().get(t))
        .map(|t| t.to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_by_label_description_and_tab() {
        let blur = search("blur");
        assert_eq!(blur[0].label, "Backdrop");
        assert_eq!(blur[0].tab, Some(1));
        let lyrics = search("lyrics size");
        assert!(lyrics.iter().any(|e| e.label == "Text size"));
        let fade = search("crossfade");
        assert!(fade.iter().all(|e| e.category == Category::Playback));
        assert!(
            search("stage")
                .iter()
                .any(|e| e.category == Category::Shortcuts)
        );
        assert!(search("nothing like this").is_empty());
    }
}
