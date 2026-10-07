//! Settings → Motion: the speed scales every duration, the switches and
//! reduced motion stop what they cover, the settings survive a restart
//! (`motion.json`), and a click in Settings changes them.

use std::time::Duration;

use gpui_kit::TestAppContext;

use super::Ui;
use crate::theme::motion::{
    self, Align, Anchor, BASE, Config, FAST, Kind, Lyrics, PageStyle, Reduce, TextSize,
};

#[gpui_kit::test]
fn speed_scales_and_instant_stops(_cx: &mut TestAppContext) {
    let at = |speed| {
        motion::set_for_test(Config {
            speed,
            ..Config::default()
        });
    };
    at(2.0);
    assert_eq!(
        motion::scaled(Kind::Panels, BASE),
        Some(Duration::from_millis(100))
    );
    at(0.5);
    assert_eq!(
        motion::scaled(Kind::Menus, FAST),
        Some(Duration::from_millis(240))
    );
    at(0.0);
    assert_eq!(motion::scaled(Kind::Panels, BASE), None);
    assert_eq!(motion::duration(BASE), None);
}

#[gpui_kit::test]
fn switches_and_reduced_motion(_cx: &mut TestAppContext) {
    motion::set_for_test(Config {
        toasts: false,
        pages: PageStyle::None,
        ..Config::default()
    });
    assert!(!motion::enabled(Kind::Toasts));
    assert!(!motion::enabled(Kind::Pages));
    assert!(motion::enabled(Kind::Menus));

    motion::set_for_test(Config {
        reduce: Reduce::Always,
        ..Config::default()
    });
    assert!(!motion::enabled(Kind::Menus));

    motion::set_desktop_for_test(true);
    motion::set_for_test(Config {
        reduce: Reduce::Never,
        ..Config::default()
    });
    assert!(motion::enabled(Kind::Menus), "Never overrides the desktop");
    motion::set_for_test(Config::default());
    assert!(!motion::enabled(Kind::Menus), "System follows the desktop");
    motion::set_desktop_for_test(false);
}

#[gpui_kit::test]
fn saves_and_loads(_cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("ytfast-motion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("motion.json");
    let config = Config {
        pages: PageStyle::Slide,
        speed: 1.5,
        skeleton: false,
        reduce: Reduce::Never,
        lyrics: Lyrics {
            anchor: Anchor::Centre,
            size: TextSize::XL,
            align: Align::Centre,
            ..Lyrics::default()
        },
        ..Config::default()
    };
    config.save(&path);
    assert_eq!(Config::load(&path), config);
    // Missing keys take their defaults; numbers out of range come back
    // inside.
    std::fs::write(&path, r#"{"speed": 99, "lyrics": {"scale": 3}}"#).unwrap();
    let loaded = Config::load(&path);
    assert_eq!(loaded.speed, 4.0);
    assert_eq!(loaded.lyrics.scale, 1.4);
    assert_eq!(loaded.pages, PageStyle::Fade);
    std::fs::write(&path, "not json").unwrap();
    assert_eq!(Config::load(&path), Config::default());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[gpui_kit::test]
fn settings_change_the_motion(cx: &mut TestAppContext) {
    motion::set_for_test(Config::default());
    let mut ui = Ui::start(cx);
    ui.keys("ctrl-,");
    reveal(&mut ui, "motion-pages:Slide");
    ui.click("motion-pages:Slide");
    assert_eq!(motion::config().pages, PageStyle::Slide);
    ui.click("motion-speed:Instant");
    assert_eq!(motion::config().speed, 0.0);
    assert!(!motion::enabled(Kind::Panels), "instant moves nothing");
    reveal(&mut ui, "lyrics-size:XL");
    ui.click("lyrics-size:XL");
    assert_eq!(motion::config().lyrics.size, TextSize::XL);
}

/// Scrolls Settings until `name` is inside the panel (Motion is below the
/// fold).
fn reveal(ui: &mut Ui, name: &str) {
    for _ in 0..40 {
        let panel = ui.find("settings");
        let el = ui.find(name);
        if el.top() > panel.top() + gpui_kit::px(60.) && el.bottom() < panel.bottom() {
            return;
        }
        ui.scroll("settings", 120.);
    }
    panic!("{name} never scrolled into Settings");
}
