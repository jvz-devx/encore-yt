//! The desktop's appearance from the XDG desktop portal's Settings interface
//! (`org.freedesktop.portal.Settings`; KDE, GNOME and others implement it):
//! light or dark (`org.freedesktop.appearance` `color-scheme`) and reduced
//! motion (`reduced-motion`, or KDE's `AnimationDurationFactor` at 0).

use std::time::Duration;

use ashpd::desktop::settings::{ColorScheme, ReducedMotion, Setting, Settings};
use smol::future::FutureExt;
use smol::stream::StreamExt;

use super::Mode;

const APPEARANCE: &str = "org.freedesktop.appearance";
const COLOR_SCHEME: &str = "color-scheme";
const REDUCED_MOTION: &str = "reduced-motion";
/// KDE's own namespace for `kdeglobals` `[KDE]`, as its portal exposes it.
const KDE: &str = "org.kde.kdeglobals.KDE";
const ANIMATION_FACTOR: &str = "AnimationDurationFactor";

/// How long the first read may hold up the window, so a missing or slow
/// portal doesn't delay startup (the dark look is used then).
const FIRST_READ: Duration = Duration::from_millis(300);

/// What the desktop asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Desktop {
    /// Light or dark; `None` when the desktop states no preference.
    pub scheme: Option<Mode>,
    /// The portal's `reduced-motion` is set.
    pub motion_reduced: bool,
    /// KDE's animation speed is set to instant.
    pub animations_off: bool,
}

impl Desktop {
    pub fn reduced_motion(&self) -> bool {
        self.motion_reduced || self.animations_off
    }

    /// Takes in one `SettingChanged` signal.
    fn change(&mut self, setting: &Setting) {
        let value = setting.value();
        match (setting.namespace(), setting.key()) {
            (APPEARANCE, COLOR_SCHEME) => {
                self.scheme = u32::try_from(value).ok().and_then(scheme_mode);
            }
            (APPEARANCE, REDUCED_MOTION) => {
                self.motion_reduced = u32::try_from(value).is_ok_and(|v| v == 1);
            }
            (KDE, ANIMATION_FACTOR) => {
                self.animations_off = <&str>::try_from(value).is_ok_and(instant);
            }
            _ => {}
        }
    }
}

/// A connection to the portal and what it said when it was opened.
pub struct Portal {
    settings: Settings,
    pub desktop: Desktop,
}

impl Portal {
    /// Connects and reads the current settings. `None` when there is no
    /// portal or it doesn't answer within [`FIRST_READ`].
    pub fn connect() -> Option<Self> {
        let open = async {
            match Settings::new().await {
                Ok(settings) => {
                    let desktop = read(&settings).await;
                    Some(Self { settings, desktop })
                }
                Err(e) => {
                    log::info!("no desktop portal settings, using the dark look: {e}");
                    None
                }
            }
        };
        let late = async {
            smol::Timer::after(FIRST_READ).await;
            log::info!("the desktop portal didn't answer in time, using the dark look");
            None
        };
        smol::block_on(open.or(late))
    }

    /// Sends the settings each time the desktop changes one of them, until
    /// the receiver is dropped or the portal goes away.
    pub async fn watch(self, changes: smol::channel::Sender<Desktop>) -> anyhow::Result<()> {
        let mut signals = self.settings.receive_setting_changed().await?;
        let mut desktop = self.desktop;
        while let Some(setting) = signals.next().await {
            let before = desktop;
            desktop.change(&setting);
            if desktop != before {
                changes.send(desktop).await?;
            }
        }
        Ok(())
    }
}

async fn read(settings: &Settings) -> Desktop {
    let scheme = match settings.color_scheme().await {
        Ok(ColorScheme::PreferDark) => Some(Mode::Dark),
        Ok(ColorScheme::PreferLight) => Some(Mode::Light),
        Ok(ColorScheme::NoPreference) | Err(_) => None,
    };
    let motion_reduced = matches!(
        settings.reduced_motion().await,
        Ok(ReducedMotion::ReducedMotion)
    );
    // Missing unless the user changed the animation speed.
    let animations_off = settings
        .read::<String>(KDE, ANIMATION_FACTOR)
        .await
        .is_ok_and(|f| instant(&f));
    Desktop {
        scheme,
        motion_reduced,
        animations_off,
    }
}

/// `color-scheme`: 1 prefers dark, 2 prefers light, 0 no preference.
fn scheme_mode(value: u32) -> Option<Mode> {
    match value {
        1 => Some(Mode::Dark),
        2 => Some(Mode::Light),
        _ => None,
    }
}

/// KDE's "Animation speed" slider at Instant writes a factor of 0.
fn instant(factor: &str) -> bool {
    factor.trim().parse::<f64>().is_ok_and(|f| f <= 0.)
}
