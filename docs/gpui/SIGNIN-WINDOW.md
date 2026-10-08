# Sign-in window spike (M31)

Windows browsers built on Chromium (Edge, Chrome, Brave) encrypt cookies under an
app-bound key no other program can open, so "Sign in with your browser" can't
read them there. The spike signs in inside a window of our own instead, on the
operating system's web engine, and reads the session cookies from that
window's own cookie store. No browser decryption is involved.

## What it is

- `crates/signin` builds `encore-yt-signin`, a separate small binary using
  `wry` 0.57 and `tao` 0.37 (both MIT or Apache-2.0). It is not a subcommand of
  `encore-yt`: on Linux wry links WebKitGTK, GTK 3 and libsoup, which would
  become a hard dependency of every package, and on every OS the main download
  would carry the web view code for a window most launches never open. The main
  binary does not change in size.
- `encore-yt-signin --out FILE` opens a 480x720 window "Sign in to YouTube
  Music" on Google's sign-in page (continuing to music.youtube.com), reads the
  webview's cookies every second (`WebView::cookies`, private browsing so
  nothing stays in the engine's profile), and once a `SAPISID` or
  `__Secure-3PAPISID` cookie exists on youtube.com (the same test as
  `auth::signs_in`) writes the YouTube and Google cookies as a Netscape cookie
  file (0600 on Unix) and exits. Stdout is only `signed in` or `cancelled`;
  closing the window is cancelling; cookie names and values are never printed.
- The app starts the helper from the Sign in sheet, waits for it, and imports
  the file through the same path as "Import a cookies file" (so it ends up as
  `imported-cookies.txt` in the config folder), then removes the temporary file.
  The row "Sign in in a window" shows wherever the helper is installed (the
  Windows and macOS installers ship it), and comes first on Windows, where the
  browser route can only read Firefox. `ENCORE_SIGNIN_WINDOW=0` hides it, `=1`
  shows it without the helper check. The helper is found next to the app, or
  at `ENCORE_SIGNIN_BIN`.
- `ENCORE_SIGNIN_URL` replaces the start page. In debug builds
  `ENCORE_SIGNIN_TEST_HOST=localhost` makes that host's cookies stand in for
  youtube.com's, for the local check below.

## Checked on Linux (WebKitGTK 2.52, Fedora, Wayland)

- Local page that sets fake SAPISID, HSID and LOGIN_INFO cookies after 3
  seconds: the helper printed `signed in`, exited 0 and wrote a correct cookie
  file with mode 0600 (captures `signin-local`).
- Through the app: sheet row, helper window, import. The log shows "saved 5
  cookies as Cookie file (imported-cookies.txt)"; YouTube then reports the fake
  session as expired, as it should. The temporary file was gone afterwards.
- Closing the window prints `cancelled`, exit 0, no file.
- Google's own page (no sign-in attempted) loads and shows the normal "Sign in"
  form with no "This browser or app may not be secure" notice. That notice
  usually appears only after submitting credentials, which could not be tried
  here, so it is still the main open question.

## Size and requirements

- Linux release build of the helper (fat LTO, opt-level s, stripped): 774 KB.
  It needs libwebkit2gtk-4.1, libgtk-3 and libsoup-3 installed; the Fedora
  and Debian desktops mostly have them, minimal installs do not. The package
  would list them as an optional dependency of the helper.
- Windows: WebView2 runtime, preinstalled on Windows 10 (recent) and 11. The
  loader is linked statically on MSVC targets (`WebView2LoaderStatic.lib`
  inside webview2-com-sys), so there is no extra DLL. `cargo xwin check
  --target x86_64-pc-windows-msvc -p encore-signin` passes. wry keeps its data
  folder under `%LOCALAPPDATA%` by default; with private browsing it holds no
  session.
- macOS: WKWebView, always present. Not built here. wry reads cookies through
  `WKWebsiteDataStore.httpCookieStore.getAllCookies` (macOS 10.13+), which
  includes HttpOnly cookies, and spins the run loop until it answers. Private
  browsing uses a non-persistent data store. tao and wry on macOS are standard
  and the code has no platform-specific parts.

## What to try on Windows and macOS

1. On the machine: `cargo build -p encore-yt -p encore-signin` (Windows may
   need `--release`), so both `encore-yt` and `encore-yt-signin` are in
   `target/<profile>/`.
2. Start the app (installed, or `target/debug/encore-yt` next to a built
   helper), signed out.
3. Sign in, then "Sign in in a window". Sign in to Google in the window,
   including any two-step prompt. Watch for "This browser or app may not be
   secure" or a block after entering the password.
4. The window should close by itself a few seconds after music.youtube.com
   loads, and the app should show your account.
5. Check the session behaves like a browser's: Library loads, a track plays,
   and it still works after restarting the app.
6. Also try closing the window early (the sheet should return to the routes).

## Risks

- Google may refuse sign-in in embedded web views. It does so mostly for
  Chromium-based embedded frameworks that identify themselves oddly; WebView2
  and WKWebView use the system engines and normal user agents, but passkeys,
  security keys and "Sign in with Google on this device" prompts can behave
  differently in a bare web view. If blocked, the fallback is the existing routes.
- The cookies belong to a sign-in made from a different client than a browser
  (user agent, device cookies): Google may ask for extra verification, or
  rotate the session sooner. Needs a longer try than a day.
- Windows machines without WebView2 (old Windows 10 builds) cannot open it.
- Another binary to ship and sign. On Linux it adds a runtime dependency on
  WebKitGTK for that one route only.
- Workspace-wide cargo commands (`--workspace`) now need the WebKitGTK and GTK 3
  development packages on Linux; `-p encore-yt` builds do not.

## Recommendation

Worth continuing if the Windows try in step 3 gets through Google's checks:
it replaces the route that cannot work on Windows with one that needs no
decryption and no browser. If Google blocks it on either platform, keep the
spike as a Linux and macOS convenience or drop it, and point Windows users to
Firefox (whose cookies are readable), the cookies file or the pasted header.
Next steps if it works: make it the first route, ship the helper in the Windows
and macOS installers, and make it an optional package on Linux.
