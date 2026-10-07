//! No XDG desktop portal outside Linux: the look stays dark unless
//! `YTFAST_GPUI_THEME` pins one, and motion is never reduced.

use super::Mode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Desktop {
    pub scheme: Option<Mode>,
}

impl Desktop {
    pub fn reduced_motion(&self) -> bool {
        false
    }
}

pub struct Portal {
    pub desktop: Desktop,
}

impl Portal {
    pub fn connect() -> Option<Self> {
        None
    }

    pub async fn watch(self, _changes: smol::channel::Sender<Desktop>) -> anyhow::Result<()> {
        Ok(())
    }
}
