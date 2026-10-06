//! Cover art: a decoded-image cache held to a memory budget, and URLs
//! asking Google's image server for the size a view draws.
//!
//! The cache keeps every image drawn in the last frame, so covers on screen
//! never reload or flicker; past the budget it lets go of the images used
//! longest ago (and frees their GPU textures).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use gpui_kit::*;

/// Decoded covers kept in memory (RGBA), in MB. A 176 px card is about
/// 120 KB at scale 1, so this holds well over a thousand on-screen-sized
/// covers. `YTFAST_GPUI_COVER_BUDGET_MB` changes it (to check eviction).
const BUDGET_MB: usize = 160;

fn budget() -> usize {
    static BUDGET: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *BUDGET.get_or_init(|| {
        std::env::var("YTFAST_GPUI_COVER_BUDGET_MB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(BUDGET_MB)
            << 20
    })
}

/// The window's scale factor (f32 bits), for the pixel size to ask for.
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

/// The cover URL for drawing `url` at `side` logical pixels. Google's image
/// server sizes on request (`=w544-h544` is what the parser asks for);
/// other URLs (video thumbnails) stay as they are.
pub fn sized(url: &str, side: Pixels) -> String {
    let scale = f32::from_bits(SCALE.load(Ordering::Relaxed)).max(1.);
    let pixels = (f32::from(side) * scale).ceil() as u32;
    match url.rfind("=w544-h544") {
        Some(at) if url.contains("googleusercontent.com") || url.contains("ggpht.com") => {
            format!(
                "{}=w{pixels}-h{pixels}{}",
                &url[..at],
                &url[at + "=w544-h544".len()..]
            )
        }
        _ => url.to_string(),
    }
}

struct Entry {
    item: ImageCacheItem,
    /// The frame that last drew it.
    used: u64,
    /// Its decoded size, once loaded.
    bytes: usize,
}

pub struct CoverCache {
    entries: HashMap<u64, Entry>,
    frame: u64,
    bytes: usize,
}

impl CoverCache {
    /// A new frame starts: past the budget, drop what no recent frame drew.
    fn frame(&mut self, window: &mut Window, cx: &mut App) {
        // The previous frame drew with `frame - 1` (layout) and `frame`
        // (paint); keep both.
        let keep_from = self.frame.saturating_sub(2);
        self.frame += 1;
        if self.bytes <= budget() {
            return;
        }
        let mut old: Vec<(u64, u64)> = self
            .entries
            .iter()
            .filter(|(_, e)| e.used < keep_from)
            .map(|(k, e)| (e.used, *k))
            .collect();
        old.sort_unstable();
        let mut freed = 0;
        let mut dropped = 0;
        for (_, key) in old {
            if self.bytes <= budget() * 3 / 4 {
                break;
            }
            if let Some(entry) = self.entries.remove(&key) {
                self.bytes -= entry.bytes;
                freed += entry.bytes;
                dropped += 1;
                if let Some(Ok(image)) = entry.item.get() {
                    cx.drop_image(image, Some(window));
                }
            }
        }
        if dropped == 0 {
            return;
        }
        log::info!(
            "cover cache: dropped {dropped} covers ({} MB), {} kept ({} MB)",
            freed >> 20,
            self.entries.len(),
            self.bytes >> 20
        );
    }
}

impl ImageCache for CoverCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let frame = self.frame;
        let entry = self.entries.entry(hash(resource)).or_insert_with(|| Entry {
            item: ImageCacheItem::new(resource, cx),
            used: frame,
            bytes: 0,
        });
        entry.used = frame;
        let result = entry.item.use_image(window);
        if entry.bytes == 0
            && let Some(Ok(image)) = &result
        {
            let size = image.size(0);
            entry.bytes = (size.width.0.max(1) * size.height.0.max(1)) as usize * 4;
            self.bytes += entry.bytes;
        }
        result
    }
}

/// The app's cover cache, made on first use.
struct Covers(Entity<CoverCache>);

impl Global for Covers {}

/// Provides the cover cache to the elements below; `root` marks the one at
/// the window's root, which starts each frame.
pub struct CoverCacheProvider {
    cache: Entity<CoverCache>,
    root: bool,
}

impl ImageCacheProvider for CoverCacheProvider {
    fn provide(&mut self, window: &mut Window, cx: &mut App) -> AnyImageCache {
        if self.root {
            SCALE.store(window.scale_factor().to_bits(), Ordering::Relaxed);
            self.cache.update(cx, |cache, cx| cache.frame(window, cx));
        }
        self.cache.clone().into()
    }
}

fn cache(cx: &mut App) -> Entity<CoverCache> {
    if let Some(covers) = cx.try_global::<Covers>() {
        return covers.0.clone();
    }
    let cache = cx.new(|_| CoverCache {
        entries: HashMap::new(),
        frame: 0,
        bytes: 0,
    });
    cx.set_global(Covers(cache.clone()));
    cache
}

/// The cover cache for the window's root element.
pub fn root_cache(cx: &mut App) -> CoverCacheProvider {
    CoverCacheProvider {
        cache: cache(cx),
        root: true,
    }
}

/// The cover cache for elements laid out outside the root's layout pass
/// (the rows of a virtual list).
pub fn nested_cache(cx: &mut App) -> CoverCacheProvider {
    CoverCacheProvider {
        cache: cache(cx),
        root: false,
    }
}
