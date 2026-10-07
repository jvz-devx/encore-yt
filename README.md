# ytfast-gpui

A native YouTube Music app for the desktop, written in Rust with [GPUI](https://www.gpui.rs), the UI framework behind the Zed editor. No Electron, no webview: GPUI draws the interface on the GPU, custom wgpu shaders draw the effects, and mpv plays the audio. It runs on Linux (built for KDE Plasma on Wayland, X11 works too), Windows and macOS.

![Now Playing: the backdrop, spectrum and waveform come from the cover and the music](docs/screenshots/gpui-now-playing.png)

It's unofficial and not affiliated with YouTube or Google. It uses YouTube Music's private web API, so a change on YouTube's side can break it. It talks only to YouTube and Google, and to LRCLIB for timed lyrics (sending a song's title, artist, album and length, nothing else). No telemetry, no server of its own.

## Download

[Releases](https://github.com/jvz-devx/ytfast-gpui/releases) has installers for each system, built by `.github/workflows/release.yml`. They're test builds (pre-releases) and aren't signed. Nothing else needs installing: the AppImage, the macOS app and the Windows installer include mpv (which plays the audio) and yt-dlp and deno (which find the streams). The app looks next to itself for them before your `PATH`, and says so under the top bar if one is missing.

- **Linux** (x86_64, glibc 2.35 or newer): the `.AppImage` (`chmod +x` it and run), or the `.deb` / `.rpm` (`sudo apt install ./ytfast-gpui-*.deb`, `sudo dnf install ./ytfast-gpui-*.rpm`). The packages use your distribution's mpv, which apt or dnf installs along with them, and add a "Music" menu entry and a `ytfast-gpui` command.
- **Windows** (x86_64): `…-setup.exe` installs for your user with a Start menu entry; `…-portable.zip` holds the same files. SmartScreen warns about the unsigned installer: More info, Run anyway.
- **macOS** (Apple silicon `macos-arm64` on macOS 14 or newer, Intel `macos-x86_64` on macOS 15 or newer): open the `.dmg` and drag ytfast to Applications. The app isn't notarized, so clear the quarantine once: `xattr -dr com.apple.quarantine /Applications/ytfast.app` (or right-click, Open).

On Windows and macOS the tray, MPRIS, notifications and following the system's light/dark setting are Linux-only for now, and closing the window quits.

**Updates:** once a day Music looks at Releases for a newer version and shows "Update available" in the top bar; Settings → Updates has Check now and the release notes. The AppImage, the Windows installer and the macOS app update themselves with Update and restart: the download must match the release's `checksums.txt`, and if the new version doesn't open its window within a minute, the previous one comes back. The `.deb`, `.rpm` and portable zip only say a new version is out, so update those the way you installed them. Pre-releases are offered while you run one (every release so far is one); Settings can turn that and the daily check off.

## What it does

- **Browse like YouTube Music:** Home with mood chips, Explore, Library (playlists, songs, albums, artists, history), album, artist, playlist and mood pages, search with suggestions and recent searches. The sidebar holds Liked music, your playlists and what you played recently.
- **Play:** gapless playback at the best quality your account gets (Opus 256 kbps with Premium), a queue you can edit and reorder, autoplay radios, timed lyrics that follow the song, Related, and loudness levelling between songs.
- **Effects:** Now Playing has a slowly flowing backdrop made from the cover, a spectrum of the music and the song's waveform; the player bar glows in the cover's colours, its seek bar shows the waveform, and covers dissolve into each other on a new song. They stop when hidden or paused and honour reduced motion.
- **Extras:** a 10-band equalizer, a sleep timer, smooth mixes (crossfades on radios), audition (hold Alt over a song to hear its best part), the most-replayed part on the seek bar, Stage (cover and big lyrics, full screen) and a mini player.
- **Your account:** likes, saving albums and playlists, subscribing, and creating, editing and deleting playlists; plays count in your history.
- **On the desktop:** media keys and the system's media controls (MPRIS), a tray icon so music keeps playing with the window closed, song-change notifications, a command line, and light and dark that follow KDE.

| | |
| --- | --- |
| ![Home](docs/screenshots/gpui-home.png) | ![Home in the light theme](docs/screenshots/gpui-home-light.png) |
| ![An album page](docs/screenshots/gpui-album.png) | ![Stage](docs/screenshots/gpui-stage.png) |

These were taken signed out (with a local test track playing), so they show public YouTube Music.

## Keyboard and command line

Press **?** for every shortcut. The common ones: **Space** play or pause, **←/→** 5 seconds back or forward, **Shift+←/→** previous or next song, **+/-** volume, **M** mute, **L** like, **S** shuffle, **R** repeat, **N** Now Playing, **Q** Up next, **F** Stage, **E** equalizer, **P** the most replayed part, **/** search, **Alt+←/→** back and forward, **Ctrl+,** Settings, **Ctrl+Q** quit. Right-click a song, album, playlist or artist (or use its **⋮**) for Play next, Add to queue, Start radio, Like, Add to playlist and more. **Ctrl+K** opens Play anything: type to find and play, or a command like `radio <artist>`, `sleep 30` or `eq bass`.

The running app takes commands:

```sh
ytfast-gpui toggle        # play or pause (also: play, pause)
ytfast-gpui next          # or: previous
ytfast-gpui like          # like or unlike the playing song
ytfast-gpui show          # bring back the window
ytfast-gpui open <link>   # a YouTube Music or YouTube link; starts the app if needed
ytfast-gpui quit
```

## Signing in

Without a sign-in source the app runs signed out and plays public YouTube Music. To use your account it reads a session it can find:

- **A browser on the same computer** (Linux): Firefox or LibreWolf, or a Chromium-family browser (Chrome, Chromium, Brave) whose key it gets from the Secret Service or KWallet. It reads the cookie store without changing it.
- **A cookie file**: a Netscape-format export in the config folder (`~/.config/ytfast/` on Linux, `%APPDATA%\ytfast\` on Windows, `~/Library/Application Support/ytfast/` on macOS) whose name contains `cookies` and ends in `.txt`, readable only by you (`chmod 600`). Export it from a private window you then close: YouTube rotates the cookies of a session that stays in use. The details, including exporting from Chromium browsers like Helium on a Mac with yt-dlp, are in [docs/integration.md](docs/integration.md).

Settings lists the sources it found; pick one if there are several. Cookie values are never logged or written anywhere others can read.

## Build from source

You need Rust 1.98 or newer, CMake, a C compiler and, on Linux, the Wayland/X11, xkbcommon, Vulkan and fontconfig development packages. At runtime: `mpv`, `yt-dlp` with its EJS challenge solver (nixpkgs' `yt-dlp` has it; Fedora's doesn't), and `deno`.

```sh
git clone https://github.com/jvz-devx/ytfast-gpui
cd ytfast-gpui/gpui
cargo run --release
```

## Development

The repository has two Cargo workspaces: the root crate (the backend, plus the original egui app behind its default feature) and `gpui/` (the GPUI app and the `ytfast-visuals` effects crate). Common commands, via [just](https://github.com/casey/just):

```sh
just check gpui          # cargo check of one crate (gpui, visuals, backend, egui)
just test backend        # one crate's tests (the parser runs against saved YouTube responses)
just verify gpui         # fmt, check, tests and clippy for a crate, before you finish
just verify-workspace    # everything, at the end
just profiling           # a release-speed build without LTO, for measurements
just shaders             # validate the WGSL shaders with naga
cd gpui && bacon         # background checks while you edit
```

`scripts/gpui-smoke.sh` builds the release app, runs it signed out and drives it on the desktop (pages, playback across track changes, a seek, Now Playing, Up next), capturing each state. `scripts/gpui-input.sh` drives KDE Wayland for visual checks. `YTFAST_FAKE_STREAM=<audio file>` plays a local file instead of YouTube streams, for checks that don't need real streams.

Further reading: [docs/gpui/PLAN.md](docs/gpui/PLAN.md) (milestones and their evidence), [gpui/DESIGN.md](gpui/DESIGN.md) (the design system), [gpui/NOTES-visuals.md](gpui/NOTES-visuals.md) (effects and their costs), [docs/gpui/BUILD-SPEED.md](docs/gpui/BUILD-SPEED.md), [AGENTS.md](AGENTS.md) (rules for coding agents, and people).

## Where it comes from

ytfast-gpui is a fork of [ytfast](https://github.com/MayberryDT/ytfast) by Tyler Mayberry (MIT), a YouTube Music player for Omarchy written with egui. This fork keeps ytfast's backend (YouTube Music's API and parser, the yt-dlp resolver, mpv playback, sign-in, MPRIS) and adds the GPUI interface, the wgpu effects, Windows and macOS builds and installers. The original egui app still builds from the root crate (`cargo run --release`); see [docs/SPEC.md](docs/SPEC.md) for its product spec.

Credits:

- [ytfast](https://github.com/MayberryDT/ytfast) by Tyler Mayberry, and through it [fastframe](https://github.com/crmne/fastframe), [ZapFast](https://github.com/crmne/zapfast) and [Spotifast](https://github.com/crmne/spotifast) by Carmine Paolino (MIT).
- [GPUI](https://github.com/zed-industries/zed) by Zed Industries and [gpui-kit / gpui-component](https://gpui-kit.com) by Longbridge (Apache-2.0).
- [mpv](https://mpv.io) plays the audio, [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [deno](https://deno.com) find the streams; they run as separate programs and the installers bundle them (licences and sources in `gpui/packaging/THIRD-PARTY.txt`).
- The [Inter](https://rsms.me/inter/) typeface (OFL) and [Lucide](https://lucide.dev) icons (ISC).

## License

[MIT](LICENSE).
