//! The keyboard map (docs/SPEC.md "Control", README "Keyboard, menus and
//! Play anything"). [`SHORTCUTS`] is what the `?` sheet lists, keys other
//! areas answer included (Stage, the equalizer, the most replayed part,
//! Audition); [`bind_keys`] binds the ones this area answers.
//!
//! Single keys bind in `Music && !Input && !MusicMenu`, so they never fire
//! while a field or a menu has the keyboard. Esc, Ctrl+K, Ctrl+Q and the
//! other Ctrl chords work from a field too. The chords use GPUI's
//! `secondary` modifier: Ctrl, or Cmd on macOS, as the keycaps say.

use gpui_kit::*;
use ytfast::backend::Command;

use crate::app::MusicApp;

actions!(
    music,
    [
        TogglePause,
        SeekBack,
        SeekForward,
        PreviousSong,
        NextSong,
        VolumeUp,
        VolumeDown,
        Mute,
        Shuffle,
        Repeat,
        NowPlaying,
        UpNext,
        FocusSearch,
        CloseLayer,
        PlayAnything,
        Shortcuts,
        Like,
        Settings,
        Quit
    ]
);

/// Where single-key shortcuts apply: not while typing, not in a menu or
/// an account dialog or Settings (M3).
const SINGLE: &str = "Music && !Input && !MusicMenu && !MusicDialog";
/// Chords that work from a field as well.
const ANYWHERE: &str = "Music";

/// Seconds the arrows seek.
const SEEK_STEP: f64 = 5.0;
/// Points `+` and `-` turn the volume.
const VOLUME_STEP: f64 = 5.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Playback,
    Library,
    Navigation,
    Views,
    /// Menus, lists and panels (M29).
    Menus,
    /// Inside Settings (M24).
    Settings,
}

impl Group {
    /// Settings → Keyboard shortcuts' tabs, in this order.
    pub const ALL: [Group; 6] = [
        Group::Playback,
        Group::Library,
        Group::Navigation,
        Group::Views,
        Group::Menus,
        Group::Settings,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Playback => "Playback",
            Group::Library => "Library",
            Group::Navigation => "Navigation",
            Group::Views => "Views",
            Group::Menus => "Menus",
            Group::Settings => "In Settings",
        }
    }
}

/// One line of the shortcuts sheet: alternative key combinations (each a
/// list of caps) and what they do. "Ctrl" is the `secondary` modifier
/// (Cmd on macOS) and "Control" the Control key itself; the keycaps name
/// them for the platform (`views::overlays::keycap`).
pub struct Shortcut {
    pub group: Group,
    pub keys: &'static [&'static [&'static str]],
    pub what: &'static str,
}

const fn line(
    group: Group,
    keys: &'static [&'static [&'static str]],
    what: &'static str,
) -> Shortcut {
    Shortcut { group, keys, what }
}

/// Every shortcut, as the sheet lists them.
pub const SHORTCUTS: &[Shortcut] = &[
    line(Group::Playback, &[&["Space"]], "Play or pause"),
    line(
        Group::Playback,
        &[&["←"], &["→"]],
        "Back or forward 5 seconds",
    ),
    line(
        Group::Playback,
        &[&["Shift", "←"], &["Shift", "→"]],
        "Previous or next song",
    ),
    line(Group::Playback, &[&["+"], &["-"]], "Volume up or down"),
    line(Group::Playback, &[&["M"]], "Mute or unmute"),
    line(Group::Playback, &[&["S"]], "Shuffle on or off"),
    line(Group::Playback, &[&["R"]], "Repeat: off, all or one"),
    line(Group::Playback, &[&["P"]], "Jump to the most replayed part"),
    line(
        Group::Playback,
        &[&["Alt"]],
        "Hold over a song to audition it",
    ),
    line(Group::Library, &[&["L"]], "Like or unlike the playing song"),
    line(Group::Library, &[&["Ctrl", "K"]], "Play anything"),
    line(Group::Library, &[&["/"], &["Ctrl", "F"]], "Search"),
    line(
        Group::Navigation,
        &[&["Alt", "←"], &["Alt", "→"]],
        "Back or forward",
    ),
    line(Group::Navigation, &[&["Esc"]], "Close what's on top"),
    line(Group::Navigation, &[&["Ctrl", "Q"]], "Quit"),
    line(Group::Views, &[&["N"]], "Open or close Now Playing"),
    line(Group::Views, &[&["Q"]], "Up next"),
    line(
        Group::Views,
        &[&["F"], &["F11"]],
        "Stage, and full screen in it",
    ),
    line(Group::Views, &[&["E"]], "Equalizer"),
    line(Group::Views, &[&["Ctrl", ","]], "Settings"),
    line(Group::Views, &[&["V"]], "Visualiser"),
    line(Group::Views, &[&["Ctrl", "M"]], "Mini player"),
    line(
        Group::Views,
        &[&["?"], &["Ctrl", "/"]],
        "Keyboard shortcuts",
    ),
    line(
        Group::Navigation,
        &[&["Tab"]],
        "Move to the next song or card, then ↑ ↓ between them",
    ),
    line(
        Group::Navigation,
        &[&["Enter"]],
        "Play the song or card in focus",
    ),
    line(
        Group::Menus,
        &[&["Menu"], &["Shift", "F10"]],
        "Open the menu of the song or card in focus",
    ),
    line(
        Group::Menus,
        &[&["↑"], &["↓"]],
        "Move through a menu, round from the end",
    ),
    line(Group::Menus, &[&["Home"], &["End"]], "First or last entry"),
    line(
        Group::Menus,
        &[&["A–Z"]],
        "Next entry starting with that letter",
    ),
    line(Group::Menus, &[&["Enter"], &["Space"]], "Choose the entry"),
    line(
        Group::Menus,
        &[&["→"], &["←"]],
        "Open or close a submenu, such as Add to playlist",
    ),
    line(
        Group::Menus,
        &[&["Esc"]],
        "Close the menu, back to where you were",
    ),
    line(
        Group::Menus,
        &[&["←"], &["→"]],
        "In the equalizer: the band before or after",
    ),
    line(
        Group::Menus,
        &[&["↑"], &["↓"], &["Shift", "↑"]],
        "In the equalizer: the band up or down 1 dB, or 3 dB",
    ),
    line(
        Group::Settings,
        &[&["↑"], &["↓"]],
        "Previous or next category",
    ),
    line(
        Group::Settings,
        &[&["Control", "Tab"], &["Control", "PageDown"]],
        "Next tab, or category where there are none",
    ),
    line(
        Group::Settings,
        &[&["Control", "Shift", "Tab"], &["Control", "PageUp"]],
        "Previous tab, or category",
    ),
    line(Group::Settings, &[&["/"]], "Search settings"),
    line(Group::Settings, &[&["Tab"]], "Next setting"),
    line(
        Group::Settings,
        &[&["←"], &["→"]],
        "Change a choice, tab or slider",
    ),
    line(
        Group::Settings,
        &[&["Shift", "←"], &["Shift", "→"]],
        "Move a slider ten steps",
    ),
    line(
        Group::Settings,
        &[&["Home"], &["End"]],
        "A slider's least or most",
    ),
    line(
        Group::Settings,
        &[&["Space"], &["Enter"]],
        "Turn a switch on or off, or press a button",
    ),
    line(
        Group::Settings,
        &[&["Esc"]],
        "Clear the search, then close Settings",
    ),
];

/// A key's name on this platform: the table's "Ctrl" is Cmd on macOS
/// (GPUI's `secondary`), "Control" the Control key, Alt is Option.
pub fn key_label(label: &'static str) -> &'static str {
    match label {
        "Ctrl" if cfg!(target_os = "macos") => "Cmd",
        "Alt" if cfg!(target_os = "macos") => "Option",
        "Control" if !cfg!(target_os = "macos") => "Ctrl",
        other => other,
    }
}

pub fn bind_keys(cx: &mut App) {
    let single = Some(SINGLE);
    let anywhere = Some(ANYWHERE);
    cx.bind_keys([
        KeyBinding::new("space", TogglePause, single),
        KeyBinding::new("left", SeekBack, single),
        KeyBinding::new("right", SeekForward, single),
        KeyBinding::new("shift-left", PreviousSong, single),
        KeyBinding::new("shift-right", NextSong, single),
        // `+` is Shift+= on most layouts; `=` alone turns it up too.
        KeyBinding::new("+", VolumeUp, single),
        KeyBinding::new("=", VolumeUp, single),
        KeyBinding::new("-", VolumeDown, single),
        KeyBinding::new("m", Mute, single),
        KeyBinding::new("s", Shuffle, single),
        KeyBinding::new("r", Repeat, single),
        KeyBinding::new("n", NowPlaying, single),
        KeyBinding::new("q", UpNext, single),
        KeyBinding::new("l", Like, single),
        KeyBinding::new("/", FocusSearch, single),
        KeyBinding::new("?", Shortcuts, single),
        KeyBinding::new("secondary-f", FocusSearch, anywhere),
        KeyBinding::new("secondary-/", Shortcuts, anywhere),
        KeyBinding::new("escape", CloseLayer, anywhere),
        KeyBinding::new("secondary-k", PlayAnything, anywhere),
        KeyBinding::new("secondary-,", Settings, anywhere),
        KeyBinding::new("secondary-q", Quit, anywhere),
    ]);
    super::menu::bind_keys(cx);
    crate::views::bind_keys(cx);
}

/// The handlers, on the window's root element.
pub fn on_actions(root: Div, cx: &mut Context<MusicApp>) -> Div {
    root.on_action(cx.listener(|this, _: &TogglePause, _, _| {
        if !this.player.queue.is_empty() {
            log::info!("key: play or pause");
            this.send(Command::TogglePause);
        }
    }))
    .on_action(cx.listener(|this, _: &SeekBack, _, cx| this.seek_by(-SEEK_STEP, cx)))
    .on_action(cx.listener(|this, _: &SeekForward, _, cx| this.seek_by(SEEK_STEP, cx)))
    .on_action(cx.listener(|this, _: &PreviousSong, _, _| {
        log::info!("key: previous song");
        this.send(Command::Previous);
    }))
    .on_action(cx.listener(|this, _: &NextSong, _, _| {
        log::info!("key: next song");
        this.send(Command::Next);
    }))
    .on_action(
        cx.listener(|this, _: &VolumeUp, window, cx| this.volume_by(VOLUME_STEP, window, cx)),
    )
    .on_action(
        cx.listener(|this, _: &VolumeDown, window, cx| this.volume_by(-VOLUME_STEP, window, cx)),
    )
    .on_action(cx.listener(|this, _: &Mute, window, cx| this.toggle_mute(window, cx)))
    .on_action(cx.listener(|this, _: &Shuffle, _, _| this.send(Command::ToggleShuffle)))
    .on_action(cx.listener(|this, _: &Repeat, _, _| this.send(Command::CycleRepeat)))
    .on_action(cx.listener(|this, _: &NowPlaying, _, cx| {
        let open = !this.player.now_playing;
        this.show_now_playing(open, cx);
    }))
    .on_action(cx.listener(|this, _: &UpNext, _, cx| this.toggle_up_next(cx)))
    .on_action(cx.listener(|this, _: &FocusSearch, window, cx| this.focus_search(window, cx)))
    .on_action(cx.listener(|this, _: &CloseLayer, window, cx| {
        if !this.close_top_layer(window, cx) {
            cx.propagate();
        }
    }))
    .on_action(
        cx.listener(|this, _: &PlayAnything, window, cx| this.toggle_play_anything(window, cx)),
    )
    .on_action(cx.listener(|this, _: &Shortcuts, window, cx| {
        let open = !this.desktop.layers.help;
        this.show_shortcuts(open, window, cx);
    }))
    .on_action(cx.listener(|this, _: &Like, _, cx| this.like_playing(cx)))
    .on_action(cx.listener(|this, _: &Settings, window, cx| this.open_settings(true, window, cx)))
    .on_action(|_: &Quit, _, cx| {
        log::info!("quitting");
        cx.quit();
    })
}
