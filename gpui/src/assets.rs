//! Icons: the Lucide icons gpui-component bundles, plus the ones it lacks.

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
        Volume2,
        VolumeX,
        ChevronLeft,
        ChevronRight,
        Music
    ]
);

/// The extra icons first (`Ok(None)` on a miss), then the bundled set, which
/// answers `Err` for anything it doesn't have and so has to go last.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(bytes) => Ok(Some(bytes)),
            None => Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut all = Assets.list(path)?;
        all.extend(ExtraIcons.list(path)?);
        Ok(all)
    }
}
