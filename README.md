# ytfast

A native YouTube Music player for [Omarchy](https://omarchy.org), written in Rust with egui. It looks and works like YouTube Music, opens in well under a second, plays audio only, and takes its colours from your Omarchy theme. In the app launcher it's called **Music**.

![Now Playing with lyrics](docs/screenshots/lyrics.png)

It's built on [fastframe](https://github.com/crmne/fastframe), [Carmine Paolino](https://github.com/crmne)'s foundation for native egui apps, and follows the pattern of his [ZapFast](https://github.com/crmne/zapfast) and [Spotifast](https://github.com/crmne/spotifast): no browser engine, no telemetry, no server of its own.

It's unofficial and not affiliated with YouTube or Google. It uses YouTube Music's private web API, so a change on YouTube's side can break it. It talks only to YouTube and Google, and to LRCLIB for timed lyrics (sending a song's title, artist, album and length, nothing else).

## GPUI app (this fork)

This fork ([jvz-devx/ytfast-gpui](https://github.com/jvz-devx/ytfast-gpui)) adds a second interface in `gpui/`: the same backend (InnerTube, yt-dlp, mpv, sign-in, MPRIS, tray) drawn with [GPUI](https://www.gpui.rs) through gpui-kit, for Fedora and KDE Plasma on Wayland (X11 should work too). It has its own design (`gpui/DESIGN.md`) instead of the Omarchy theme. The plan and its progress are in [docs/gpui/PLAN.md](docs/gpui/PLAN.md).

![The GPUI app's Home, signed out](docs/screenshots/gpui-home.png)

### Download and install

[Releases](https://github.com/jvz-devx/ytfast-gpui/releases) carry installers built by `.github/workflows/release.yml`. Each bundles `yt-dlp` and `deno` (the app puts its own directory first on `PATH`, so they win over the system's); the Windows installer also brings `mpv`. If a tool is missing, the app says so under the top bar.

- **Linux** (x86_64, glibc 2.35 or newer): `ytfast-gpui-<version>-linux-x86_64.AppImage` (`chmod +x` it and run; needs `mpv` installed, e.g. `sudo dnf install mpv` or `sudo apt install mpv`), or the `.deb` / `.rpm` (`sudo apt install ./ytfast-gpui-*.deb`, `sudo dnf install ./ytfast-gpui-*.rpm`), which pull in `mpv` and put the app in `/usr/lib/ytfast-gpui` with a `ytfast-gpui` command and a "Music" menu entry.
- **Windows** (x86_64): `ytfast-gpui-<version>-windows-x86_64-setup.exe` installs for your user (no administrator rights) with a Start menu entry; the `-portable.zip` holds the same files. The installer isn't signed, so SmartScreen warns: More info, Run anyway.
- **macOS** (Apple silicon `macos-arm64`, Intel `macos-x86_64`): open the `.dmg` and drag ytfast to Applications (named so it doesn't clash with Apple's Music). Install mpv with `brew install mpv`. The app isn't signed or notarized, so clear the quarantine once before opening it: `xattr -dr com.apple.quarantine /Applications/ytfast.app` (or right-click it, Open, and confirm).

On Windows and macOS the tray, MPRIS, notifications, signal handling and the light/dark portal are Linux-only for now (closing the window quits there), and sign-in reads only a cookie file (see [From another computer](#from-another-computer-a-cookie-file)): put it in `%APPDATA%\ytfast\` on Windows or `~/Library/Application Support/ytfast/` on macOS. Browser profiles are read on Linux only.

### Build and run

```sh
cd gpui && cargo run --release
```

`gpui/` is its own Cargo workspace; the root crate (the egui app) builds as before.

At runtime it needs:

- `mpv` for playback.
- `yt-dlp` with the EJS challenge solver, plus `deno`, for streams. Fedora's `yt-dlp` lacks the solver, so signed-in streams fail with "Requested format is not available"; nixpkgs' `yt-dlp` bundles it (`nix profile add nixpkgs#yt-dlp nixpkgs#deno`, ahead of `/usr/bin` in `PATH`).
- For your account, one sign-in source: an exported cookie file at `~/.config/ytfast/*cookies*.txt` (mode 0600), a Firefox or LibreWolf profile signed in to YouTube, or a Chromium-family browser (Chrome, Chromium, Brave) whose key comes from the Secret Service or KWallet. Without one it runs signed out.

Light and dark follow the desktop while the app runs (the XDG desktop portal's colour scheme, which KDE sets from its colour scheme), and so does reduced motion (KDE's animation speed at Instant). `YTFAST_GPUI_THEME=light` or `=dark` pins a look.

Scripts:

- `scripts/check.sh gpui|backend|egui|tests|shaders`: a quick `cargo check` of what you touched.
- `scripts/gpui-check.sh`: the gate: formatting, clippy for both crates, tests and a release build.
- `scripts/gpui-smoke.sh`: builds the release app and runs it signed out with a fresh state, visits Home, Explore, a mood, an album, search and an artist, plays three songs across Next and a seek, opens Now Playing (Lyrics) and Up next, and captures each state to `artifacts/gpui/smoke-*.png`. It checks the log and mpv and exits non-zero on a failure.
- `scripts/gpui-input.sh`: drives the desktop for visual checks (KWin, ydotool, spectacle): launch, click, keys, text, captures, and a lock so one script drives the screen at a time.

## Screenshots

| | |
| --- | --- |
| ![An artist page, with a song playing](docs/screenshots/artist.png) | ![An album page](docs/screenshots/album.png) |
| ![Explore: new releases, moods and genres](docs/screenshots/explore.png) | ![Search results](docs/screenshots/search.png) |

Colours come from your Omarchy theme and change with it while the app is open. Here's the same album in a light theme:

![The album page in a light Omarchy theme](docs/screenshots/album-light-theme.png)

These were taken signed out, so they show public YouTube Music rather than anyone's library, and the corner says "Signed out of YouTube Music". Signed in, Home, Library and the sidebar show your own music.

## What it does

The YouTube Music you know, with its own feel:

- **Covers that fly.** Open an album and its cover lifts out of the card into the page; Back sends it home. Songs fly into the player, and Now Playing opens out of it.
- **Song changes you see but never hear.** Playback is gapless; at the change the next cover and title roll into the player.
- **Now Playing** with lyrics that follow the song line by line (from YouTube Music, or [LRCLIB](https://lrclib.net) when it has none). Click a line to jump there.
- **Stage** (`F`): the cover and large lyrics fill the window. For a second screen or a party.
- **The most replayed part**: a ridge along the seek bar shows where everyone replays a song, with a jump to the peak.
- **Audition**: hold `Alt` (or the middle button) on any song to hear its best part over your music, which dips and comes back. Your queue never changes.
- **Smooth mixes**: radios and mixes can blend from song to song. Albums stay gapless.
- **Theme-painted covers**: an optional mode draws every cover in your Omarchy theme's colours.
- **Things with weight**: cards lift, play turns into pause, carousels glide and settle on a card.

And everything you'd expect:

- Home (with its moods), Explore, Library (with History), search with suggestions and recent searches, album, artist and playlist pages
- A queue you can edit: Play next, Add to queue, drag to reorder, remove, clear
- Likes and dislikes, saving albums and playlists to your library, subscribing to artists, and creating, editing and deleting your playlists
- Audio at the best quality your account gets (Opus at about 256 kbps with YouTube Music Premium), levelled between songs, with a ten-band equalizer and a sleep timer
- Songs on screen are prepared before you click them, so they start at once; Music reopens where you left off
- Plays count in your YouTube Music history, so your recommendations keep learning
- Media keys, `playerctl` and the Omarchy bar's media widget (MPRIS), playing on after you close the window, a command line, a mini player, and song-change notifications if you want them

## Keyboard, menus and Play anything

Press **?** to see every shortcut. The common ones: **Space** play or pause, **←/→** back or forward 5 seconds, **Shift+←/→** previous or next song, **+/-** volume, **M** mute, **L** like, **S** shuffle, **R** repeat, **N** Now Playing, **Q** Up next, **F** Stage (**F11** full screen inside it), **P** the most replayed part, **/** search, **Alt+←/→** back and forward, **E** equalizer, **Ctrl+,** Settings, **Esc** closes what's on top. Hold **Alt** over a song to audition it. They don't fire while you type in a field.

Right-click a song, album, playlist or artist (or use its **⋮**) for Play next, Add to queue, Start radio, Like, Add to playlist, Save to library, Go to album or artist and Copy link. Menus work from the keyboard too: arrows, Enter, Esc.

**Ctrl+K** opens Play anything: type and it finds your library, recent searches, the page you're on and YouTube Music; Enter plays the top match, Shift+Enter opens its page. It takes commands as well: `radio <song or artist>`, `like`, `next`, `pause`, `play`, `shuffle`, `repeat`, `sleep 30` (or `sleep end`), `eq bass`, `mini`.

**Save** in Up next makes the queue a playlist.
## On the desktop

Closing the window keeps the music playing, with a Music icon in the bar's tray: click it to bring the window back where you left it, middle-click to play or pause, scroll to change the volume, or right-click for Next, Previous and Quit. Launching Music again, `ytfast show` or the media widget also bring it back. **Ctrl+Q** quits and stops the music; closing the window with nothing queued quits too.

The running app takes commands, for Hyprland bindings and scripts:

```sh
ytfast toggle            # play or pause (also: play, pause)
ytfast next              # or: previous
ytfast like              # like or unlike the playing song
ytfast show              # bring back the window
ytfast open <link>       # a YouTube Music or YouTube link; starts Music if it isn't running
ytfast quit              # quit and stop the music
```

Links open in ytfast too when you paste one into the search field. Songs start playing; albums, artists, playlists and searches open their page. Dropping a link file on the window works under X11, but not on Wayland: the windowing library ytfast uses doesn't receive drops there yet.

**Ctrl+M** (or the button next to the volume) switches to the mini player, a small window with the cover, the song, a progress bar and the controls. Its button on the right brings back the full window. To keep it floating above other windows in Hyprland:

```ini
windowrule = float, class:ytfast-mini
windowrule = pin, class:ytfast-mini
```

Song-change notifications are off by default; turn them on in **Settings**. They don't appear while a ytfast window has the focus.

## How it signs in

ytfast reads your YouTube sign-in from a browser you're already signed in to, or from a cookie file exported on another computer (below): Firefox or LibreWolf (including their Flatpaks), or a Chromium-family browser (Brave, Brave Origin, Google Chrome or Chromium). It reads the browser's cookie store without changing it. Chromium browsers encrypt it, so ytfast decrypts it with the key the browser keeps in your keyring (the Secret Service, or KWallet on KDE); Firefox doesn't. You never paste headers, and on this computer you don't export files. In Firefox, sign in outside container tabs: ytfast reads only the default container.

By default it uses the browser profile you used most recently. If different browsers are signed in to different Google accounts, choose one in **Settings**; ytfast remembers it in `~/.config/ytfast/settings.json`.

### From another computer: a cookie file

If the browser you use YouTube Music in is on another computer (a Mac, say), export its session as a Netscape cookie file and put it in `~/.config/ytfast/` with a name containing `cookies` and ending in `.txt` (`cookies.txt`, `browser-cookies.txt`). ytfast lists each such file in **Settings** as "Cookie file (cookies.txt)" next to the browser profiles, and uses it for YouTube Music and for `yt-dlp` like a browser's cookies. It reads only a file's youtube.com and google.com lines, and refuses a file unless it belongs to you and only you can read it (mode `0600`).

YouTube rotates the cookies of a session that stays in use, and a copy of them soon stops working. So export a session that won't be used again:

- **Private window (recommended):** open a private window, sign in to music.youtube.com, export the youtube.com cookies with a cookies.txt extension (yt-dlp suggests "Get cookies.txt LOCALLY" for Chromium browsers and "cookies.txt" for Firefox), then close the private window.
- **With yt-dlp:** `yt-dlp --cookies-from-browser` reads a browser's saved cookies, not a private window's. Use a separate browser profile: sign in to music.youtube.com there, quit the browser, export, and don't open that profile again. yt-dlp ends with "You must provide at least one URL" but writes the file anyway; the file holds every site's cookies from that profile, so delete it on the Mac once copied.

  ```sh
  yt-dlp --cookies-from-browser chrome:"Profile 2" --cookies cookies.txt    # Google Chrome
  yt-dlp --cookies-from-browser brave:"Profile 2" --cookies cookies.txt     # Brave
  yt-dlp --cookies-from-browser safari --cookies cookies.txt                # Safari
  ```

  On macOS yt-dlp asks the Keychain for the browser's "Safe Storage" item (`Chrome Safe Storage`, `Brave Safe Storage`; macOS asks you to allow it). Safari needs no Keychain item, but the terminal needs Full Disk Access. For another Chromium-based browser, give yt-dlp the profile's path: `chromium:"$HOME/Library/Application Support/<browser>/<profile>"`. It then looks for the item `Chromium Safe Storage` (account `Chromium`). Helium (profiles in `~/Library/Application Support/net.imput.helium`) files its key as `Helium Storage Key` (account `Helium`), so add a `Chromium Safe Storage` item with the same password for the export and delete it afterwards (this clashes with a real Chromium install, which has its own item):

  ```sh
  security add-generic-password -a Chromium -s "Chromium Safe Storage" \
    -w "$(security find-generic-password -w -a Helium -s 'Helium Storage Key')"
  yt-dlp --cookies-from-browser chromium:"$HOME/Library/Application Support/net.imput.helium/Profile 2" --cookies cookies.txt
  security delete-generic-password -a Chromium -s "Chromium Safe Storage"
  ```

Copy it here with the right mode, then press Reconnect (or restart ytfast):

```sh
scp mac:cookies.txt ~/.config/ytfast/cookies.txt
chmod 600 ~/.config/ytfast/cookies.txt
```

When YouTube Music says the session has expired, export a fresh one the same way.

Cookies are never logged or written anywhere readable by other users. To see what ytfast can read from each profile (counts only, no values), run `cargo run --example sign_in --no-default-features`. While it runs, the cookie file that `yt-dlp` needs lives in `$XDG_RUNTIME_DIR/ytfast` with permissions `0600`.

## Install

You need:

- to build: Rust 1.98 or newer, CMake and a C compiler
- to run: `mpv`, `yt-dlp`, `deno` (yt-dlp uses it for YouTube's player challenges) and, for Chromium-family browsers, `secret-tool` (libsecret) or, on KDE, KWallet

```sh
git clone https://github.com/MayberryDT/ytfast
cd ytfast
cargo build --release
install -Dm755 target/release/ytfast ~/.local/bin/ytfast
install -Dm644 assets/ytfast.desktop ~/.local/share/applications/ytfast.desktop
```

The release build takes several minutes and a few GB of memory. Logs go to `~/.cache/ytfast/ytfast.log`.

## Development

[docs/SPEC.md](docs/SPEC.md) describes the product: what each screen does and what's deliberately left out. [docs/integration.md](docs/integration.md) has the verified facts it relies on (cookie decryption, YouTube's API, stream formats) and the design. [AGENTS.md](AGENTS.md) holds the rules for coding agents, and for people too.

`scripts/e2e.sh [journey|recovery|offline|theme|showcase|motion|pages|desktop|account|engine|engine-restore|surfaces|deck]` builds with the `e2e` feature and drives the real app on your desktop. All but `showcase` use your signed-in account and leave screenshots, logs and a summary in `artifacts/e2e/`, which git ignores because they show account data. `showcase` runs signed out and takes the pictures above. `desktop` also needs `playerctl` and a notification daemon. `account` changes your account and puts it back; `engine` must be followed by `engine-restore`.

Issues and pull requests are welcome.

## Credits

- [fastframe](https://github.com/crmne/fastframe) by Carmine Paolino (MIT): the theme, fonts, icons, text, logging and shell crates, and his forks of [egui](https://github.com/crmne/egui) and [winit](https://github.com/crmne/winit). ytfast copies the shape of [ZapFast](https://github.com/crmne/zapfast) and [Spotifast](https://github.com/crmne/spotifast); its MPRIS service follows Spotifast's.
- [egui](https://github.com/emilk/egui) by Emil Ernerfeldt and contributors.
- [mpv](https://mpv.io) plays the audio and [yt-dlp](https://github.com/yt-dlp/yt-dlp) finds the streams; ytfast runs both as separate programs.
- Icons from [Lucide](https://lucide.dev) (ISC, see `assets/icons/LICENSE.txt`).

## License

[MIT](LICENSE).
