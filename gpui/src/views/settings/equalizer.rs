//! Settings → Equalizer: on or off, the presets, and the equalizer panel
//! (E) for the bands themselves.

use gpui_kit::assets::IconName;
use gpui_kit::component::h_flex;
use gpui_kit::*;
use ytfast::equalizer::{Equalizer, Preset};

use super::super::widgets::{self, Pill};
use crate::app::MusicApp;
use crate::theme::{Colors, size};

pub fn section(app: &MusicApp, c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    let eq = app.equalizer();
    let detail = match (eq.enabled, eq.preset) {
        (false, _) => "Songs play as they were mixed".to_string(),
        (true, Preset::Flat) => "On, with every band flat".to_string(),
        (true, preset) => format!("{} shapes every song", preset.label()),
    };
    let switch = widgets::switch("eq-enabled", eq.enabled, c).on_click(cx.listener(
        |this, on: &bool, _, cx| {
            let eq = this.equalizer();
            this.set_equalizer(Equalizer { enabled: *on, ..eq }, cx);
        },
    ));
    // Custom bands show as a choice of their own, only while they're set.
    let custom = (eq.preset == Preset::Custom).then_some(Preset::Custom);
    let presets = Preset::ALL.into_iter().chain(custom).map(|preset| {
        let chosen = eq.enabled && eq.preset == preset;
        super::choice(("eq-preset", preset as usize), preset.label(), chosen, c).on_click(
            cx.listener(move |this, _, _, cx| {
                if preset == Preset::Custom {
                    let eq = this.equalizer();
                    this.set_equalizer(
                        Equalizer {
                            enabled: true,
                            ..eq
                        },
                        cx,
                    );
                } else {
                    this.eq_preset(preset, cx);
                }
            }),
        )
    });
    super::section(
        "Equalizer",
        c,
        [
            super::row("Use the equalizer", Some(detail.into()), switch, c),
            super::choices(presets),
            open_panel(c, cx),
        ],
    )
}

/// Closes Settings and opens the equalizer panel above the player bar.
fn open_panel(c: &Colors, cx: &mut Context<MusicApp>) -> AnyElement {
    h_flex()
        .child(
            widgets::pill_button(
                "settings-eq-panel",
                "Adjust bands",
                Some(widgets::icon(
                    IconName::SlidersVertical,
                    size::ICON_SM,
                    c.text,
                )),
                Pill::Secondary,
                c,
            )
            .tooltip(widgets::tooltip("Opens the equalizer (E)"))
            .on_click(cx.listener(|this, _, window, cx| {
                this.open_settings(false, window, cx);
                if !this.extras.equalizer_open {
                    this.toggle_equalizer(window, cx);
                }
            })),
        )
        .into_any_element()
}
