//! Bundled assets: the Lucide icons gpui-component ships plus the ones it
//! lacks, our filled transport glyphs, and the Inter fonts.

use std::borrow::Cow;

use gpui_kit::assets::Assets;
use gpui_kit::*;

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        House,
        Compass,
        LibraryBig,
        Search,
        SkipBack,
        SkipForward,
        Play,
        Pause,
        Shuffle,
        Repeat,
        Repeat1,
        Volume,
        Volume1,
        Volume2,
        VolumeX,
        ChevronLeft,
        ChevronRight,
        Music,
        AudioLines,
        Radio,
        ListMusic,
        MicVocal,
        EllipsisVertical,
        CircleAlert,
        RefreshCw,
        Disc3,
        UserRound,
        Infinity,
        X,
        Copy,
        ChevronDown,
        ListX,
        RotateCcwClock,
        Moon,
        SlidersVertical,
        PictureInPicture2,
        Maximize2,
        Minimize2,
        Minus,
        Plus,
        ListStart,
        ListEnd,
        Link,
        Command,
        Check,
        ThumbsUp,
        ThumbsDown,
        ListPlus,
        Plus,
        Pencil,
        Trash,
        Settings,
        LogIn,
        RefreshCcw,
        Bell,
        AudioWaveform,
        WifiOff,
        CircleUserRound,
        Lock
    ]
);

/// Filled glyphs for the transport and play buttons (Lucide's are outlines).
/// Use them through [`Glyph`].
const GLYPHS: &[(&str, &[u8])] = &[
    (
        "icons/fill/play.svg",
        include_bytes!("../assets/icons/fill/play.svg"),
    ),
    (
        "icons/fill/pause.svg",
        include_bytes!("../assets/icons/fill/pause.svg"),
    ),
    (
        "icons/fill/skip-back.svg",
        include_bytes!("../assets/icons/fill/skip-back.svg"),
    ),
    (
        "icons/fill/skip-forward.svg",
        include_bytes!("../assets/icons/fill/skip-forward.svg"),
    ),
    (
        "icons/fill/thumbs-up.svg",
        include_bytes!("../assets/icons/fill/thumbs-up.svg"),
    ),
    (
        "icons/fill/thumbs-down.svg",
        include_bytes!("../assets/icons/fill/thumbs-down.svg"),
    ),
];

/// A filled glyph from [`GLYPHS`].
#[derive(Clone, Copy, Debug)]
pub enum Glyph {
    Play,
    Pause,
    SkipBack,
    SkipForward,
    /// A song that is liked.
    ThumbsUp,
    /// A song that is disliked.
    ThumbsDown,
}

impl Glyph {
    pub fn path(self) -> &'static str {
        match self {
            Glyph::Play => "icons/fill/play.svg",
            Glyph::Pause => "icons/fill/pause.svg",
            Glyph::SkipBack => "icons/fill/skip-back.svg",
            Glyph::SkipForward => "icons/fill/skip-forward.svg",
            Glyph::ThumbsUp => "icons/fill/thumbs-up.svg",
            Glyph::ThumbsDown => "icons/fill/thumbs-down.svg",
        }
    }
}

/// Inter 4.001 (SIL OFL 1.1, `assets/fonts/Inter-LICENSE.txt`) as static
/// cuts made by `assets/fonts/instance.py`: the text cut in four weights and
/// the display cut in bold.
pub const FONTS: &[(&str, &[u8])] = &[
    (
        "fonts/Inter-Regular.ttf",
        include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    ),
    (
        "fonts/Inter-Medium.ttf",
        include_bytes!("../assets/fonts/Inter-Medium.ttf"),
    ),
    (
        "fonts/Inter-SemiBold.ttf",
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    ),
    (
        "fonts/Inter-Bold.ttf",
        include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    ),
    (
        "fonts/InterDisplay-Bold.ttf",
        include_bytes!("../assets/fonts/InterDisplay-Bold.ttf"),
    ),
];

/// Our files and the extra icons first (`Ok(None)` on a miss), then the
/// bundled set, which answers `Err` for anything it doesn't have and so has
/// to go last.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = GLYPHS.iter().chain(FONTS).find(|(p, _)| *p == path) {
            return Ok(Some(Cow::Borrowed(*bytes)));
        }
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = Assets.list(path)?;
        all.extend(ExtraIcons.list(path)?);
        all.extend(
            GLYPHS
                .iter()
                .chain(FONTS)
                .map(|(p, _)| *p)
                .filter(|p| p.starts_with(path))
                .map(SharedString::from),
        );
        Ok(all)
    }
}
