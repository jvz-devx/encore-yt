//! M25: which songs are liked, for every row that marks them.
//!
//! The one source is the backend crate's [`Marks`]: fresh pages (the Liked
//! music list among them) and watch-next say what each song's rating is,
//! a like or unlike made here shows at once, and a row whose song none of
//! those mention falls back to the rating its own response carried. Rows
//! read it through [`Likes`], so every visible copy of a song agrees.
//!
//! For checks signed out (no song is liked then),
//! `YTFAST_GPUI_FAKE_LIKED=<videoId,videoId,…>` marks those songs liked, and
//! `YTFAST_GPUI_FAKE_LIKED=every<N>` (e.g. `every3`) about one song in N.

use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use ytfast::account::Marks;
use ytfast::model::{LikeStatus, Track};

use crate::app::MusicApp;

/// What rows need to mark liked songs; cheap to make each frame.
#[derive(Clone)]
pub struct Likes {
    marks: Arc<Marks>,
    /// Rows have a like slot: signed in, or the test hook is on.
    shown: bool,
}

impl Likes {
    pub fn of(app: &MusicApp) -> Self {
        Self {
            marks: app.account.state.marks.clone(),
            shown: app.account.signed_in() || fake().is_some(),
        }
    }

    /// Whether song rows keep a slot for the like mark.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// Whether `track` is in Liked music, as the app shows it.
    pub fn liked(&self, track: &Track) -> bool {
        fake().is_some_and(|f| f.likes(&track.video_id))
            || self.marks.like(track) == LikeStatus::Like
    }
}

/// The test hook's songs.
enum Fake {
    Ids(HashSet<String>),
    Every(u32),
}

impl Fake {
    fn likes(&self, video_id: &str) -> bool {
        match self {
            Fake::Ids(ids) => ids.contains(video_id),
            // A stable spread over the ids, so a song looks the same in
            // every list it appears in.
            Fake::Every(n) => {
                video_id
                    .bytes()
                    .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
                    % n
                    == 0
            }
        }
    }
}

fn fake() -> Option<&'static Fake> {
    static FAKE: OnceLock<Option<Fake>> = OnceLock::new();
    FAKE.get_or_init(|| {
        let value = std::env::var("YTFAST_GPUI_FAKE_LIKED").ok()?;
        let value = value.trim();
        if let Some(n) = value.strip_prefix("every") {
            return n.parse().ok().filter(|n| *n > 0).map(Fake::Every);
        }
        let ids: HashSet<String> = value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        (!ids.is_empty()).then_some(Fake::Ids(ids))
    })
    .as_ref()
}
