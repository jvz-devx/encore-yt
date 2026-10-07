#!/usr/bin/env bash
# Smoke test of the GPUI app (docs/gpui/PLAN.md M7): builds the release
# binary, runs it signed out with a fresh state, visits each page, plays
# three songs across track changes and a seek, and captures each state to
# artifacts/gpui/smoke-*.png. Exits non-zero when any check fails.
#
#   scripts/gpui-smoke.sh            build, then drive the desktop (under the
#                                    desktop lock of scripts/gpui-input.sh)
#   SMOKE_NO_BUILD=1 scripts/...     skip the build (use the last release build)
#
# Signed out and fresh: XDG_CONFIG_HOME, XDG_CACHE_HOME and XDG_RUNTIME_DIR
# point at a temporary directory, so no cookie file, Chromium profile, saved
# session or resolved stream of another run is found. Wayland, PipeWire,
# PulseAudio and D-Bus are passed by full path. Firefox profiles under
# ~/.mozilla are still read; the run checks that the app stays signed out.
#
# Click targets are fixed layout positions (sidebar items, the search field,
# the player bar, Explore's first album and mood tile, a page header's Play,
# search's top result), measured with the window at 0,0 1280x1000 as
# `scripts/gpui-input.sh launch` places it. Home's content changes from day
# to day, so nothing on it is clicked. Shortcuts would be steadier where
# they exist; on main today only Alt+Left/Right and Ctrl+Q do. Worth using
# once they land: Space (play), Shift+Right (next), N (Now Playing),
# Q (Up next), / (search), and a key to open the first result.
set -euo pipefail
cd "$(dirname "$0")/.."
input=scripts/gpui-input.sh

if [ -z "${SMOKE_LOCKED:-}" ]; then
    if [ -z "${SMOKE_NO_BUILD:-}" ]; then
        nice cargo build -j "${JOBS:-3}" -p ytfast-gpui --release
    fi
    export SMOKE_LOCKED=1
    exec "$input" locked "$0" "$@"
fi

started=$(date +%s)
# A short path: the app's sockets live under it (sun_path is 108 bytes).
state="$(mktemp -d /tmp/ytfast-smoke.XXXXXX)"
log="$state/cache/ytfast/ytfast-gpui.log"
desktop_run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
wayland="${WAYLAND_DISPLAY:-wayland-0}"
[[ "$wayland" == /* ]] || wayland="$desktop_run/$wayland"
failures=0
shots=0

# The app's file name must stay ytfast-gpui (`gpui-input.sh stop` finds it so).
mkdir -p "$state/bin" "$state/config" "$state/cache" "$state/run" artifacts/gpui
chmod 700 "$state/run"
cp target/release/ytfast-gpui "$state/bin/ytfast-gpui"
rm -f artifacts/gpui/smoke-*.png

finish() {
    "$input" stop
    rm -rf "$state"
}
trap finish EXIT

pass() { printf '  ok    %s\n' "$*"; }
fail() {
    printf '  FAIL  %s\n' "$*"
    failures=$((failures + 1))
}

# shot NAME WHAT: captures the window to artifacts/gpui/smoke-NN-NAME.png.
shot() {
    shots=$((shots + 1))
    local file
    file="$(printf 'smoke-%02d-%s' "$shots" "$1")"
    # spectacle has hung here once; don't let one capture stall the run.
    if timeout 20 "$input" shot "$file" full >/dev/null ||
        timeout 20 "$input" shot "$file" full >/dev/null; then
        magick "artifacts/gpui/$file.png" -crop 1280x1000+0+0 +repage "artifacts/gpui/$file.png"
        printf '%-40s %s\n' "artifacts/gpui/$file.png" "$2"
    else
        fail "capture $file failed"
    fi
}

click() { "$input" click "$1" "$2"; }
key() { "$input" key "$@"; }
pause() { sleep "$1"; }

# wait_log PATTERN SECONDS: waits until the app log matches (extended regex).
wait_log() {
    local deadline=$((SECONDS + $2))
    while [ "$SECONDS" -lt "$deadline" ]; do
        grep -qE "$1" "$log" 2>/dev/null && return 0
        sleep 0.5
    done
    return 1
}

# Lines of the app log matching PATTERN.
count_log() { grep -cE "$1" "$log" 2>/dev/null || true; }

# Whether a ytfast-gpui process runs (as `gpui-input.sh stop` finds it).
app_running() {
    local p e
    for p in /proc/[0-9]*; do
        e="$(readlink "$p/exe" 2>/dev/null)" || continue
        case "${e##*/}" in ytfast-gpui*) return 0 ;; esac
    done
    return 1
}

# Layout positions (screen pixels; the client area starts below a ~30 px
# title bar).
NAV_HOME=(80 124)
NAV_EXPLORE=(80 168)
SEARCH=(515 67)            # the search field
FIRST_MOOD=(350 599)       # Explore: the first tile of Moods & genres
FIRST_ALBUM=(351 343)      # Explore: the first card of New albums & singles
HEADER_PLAY=(560 329)      # a page header's Play pill (bottom-aligned with the cover)
FIRST_RESULT=(351 330)     # search: the top result
NEXT=(689 943)             # player bar
SEEK_MID=(640 976)         # the seek bar's middle
PLAYER_SONG=(150 955)      # player bar song: opens and closes Now Playing
LYRICS_TAB=(1029 117)      # Now Playing's Lyrics tab
UP_NEXT=(1097 946)         # player bar: Up next

echo "== smoke: release build, signed out, fresh state in $state"
"$input" setup
# Its own runtime directory too (resolved streams and the single-instance
# socket), with Wayland, PipeWire, PulseAudio and D-Bus named by full path.
XDG_CONFIG_HOME="$state/config" XDG_CACHE_HOME="$state/cache" \
    XDG_RUNTIME_DIR="$state/run" \
    WAYLAND_DISPLAY="$wayland" \
    PIPEWIRE_RUNTIME_DIR="$desktop_run" PULSE_SERVER="unix:$desktop_run/pulse/native" \
    DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$desktop_run/bus}" \
    "$input" launch "$state/bin/ytfast-gpui"

echo "== pages"
# Never drive a signed-in app: plays would land in someone's history.
if ! wait_log 'account: (signed|unverified)' 15; then
    fail "the account check didn't finish"
elif grep -q 'account: signed in' "$log"; then
    echo "  FAIL  the app signed in (a browser profile?); stopping" >&2
    exit 1
fi
pause 2
shot home "Home, signed out: mood chips and the first shelves"
"$input" move 700 500
# On KWin a negative wheel count scrolls the page down.
"$input" scroll -6
pause 1.5
shot home-scrolled "Home scrolled down: later shelves"

click "${NAV_EXPLORE[@]}"
pause 3
shot explore "Explore: New releases, Charts, Moods & genres; new albums; moods"

click "${FIRST_MOOD[@]}"
pause 3
shot mood "A mood page (Explore's first mood tile): its shelves of playlists"

key 56:1 105:1 105:0 56:0   # Alt+Left: back to Explore
pause 2
click "${FIRST_ALBUM[@]}"
pause 3
shot album "An album page (Explore's first new album): header, Play, songs"

echo "== playback"
click "${HEADER_PLAY[@]}"
if wait_log 'now playing [A-Za-z0-9_-]{11} ' 20; then
    pass "the first song started"
else
    fail "no song started within 20 s"
fi
pause 2
shot playing "Song 1 playing: the player bar shows it, its row in signal"
[ "$(count_log "decode [0-9]+: .* Hz")" -gt 0 ] && pass "the audio engine decodes" || fail "the audio engine decoded nothing"

click "${NEXT[@]}"
wait_log 'now playing .*\(queue 2\)' 15 && pass "Next: song 2" || fail "Next didn't reach song 2"
pause 3
click "${NEXT[@]}"
wait_log 'now playing .*\(queue 3\)' 15 && pass "Next: song 3" || fail "Next didn't reach song 3"
pause 3
click "${SEEK_MID[@]}"
if wait_log 'seek to [0-9.]+s' 5; then
    to="$(grep -oE 'seek to [0-9.]+' "$log" | tail -1 | grep -oE '[0-9.]+$')"
    pause 1.5
    # The engine's decoder logs where the seek landed.
    at="$(grep -oE 'seek to [0-9.]+s landed at [0-9.]+s' "$log" | tail -1 | grep -oE '[0-9.]+s$' | tr -d s)"
    at="${at:-0}"
    if awk -v a="$at" -v t="$to" 'BEGIN { exit !(a >= t - 1 && a <= t + 5) }'; then
        pass "seek to ${to}s, the decoder at ${at}s"
    else
        fail "seek to ${to}s, but the decoder is at ${at}s"
    fi
else
    fail "the seek bar click didn't seek"
fi
shot seeked "Song 3 after the seek: the bar near its middle"

echo "== now playing and up next"
click "${PLAYER_SONG[@]}"
pause 2
click "${LYRICS_TAB[@]}"
pause 4
shot now-playing-lyrics "Now Playing, Lyrics tab (lyrics, or a plain 'no lyrics')"
click "${PLAYER_SONG[@]}"
pause 1
click "${UP_NEXT[@]}"
pause 2
shot up-next "Up next panel: the queue with song 3 playing"
click "${UP_NEXT[@]}"
pause 1

echo "== search and artist"
click "${SEARCH[@]}"
pause 0.5
"$input" type "daft punk"
pause 0.5
key 28:1 28:0
pause 4
shot search "Search results for 'daft punk'"
click "${FIRST_RESULT[@]}"
pause 4
shot artist "Artist page (the top result): header, songs, albums"

click "${NAV_HOME[@]}"
pause 2
shot home-again "Back on Home, the song still playing"

echo "== log and processes"
resolved=$(count_log 'resolved [A-Za-z0-9_-]{11} .*itag [0-9]+')
[ "$resolved" -ge 3 ] && pass "$resolved streams resolved with an itag" ||
    fail "only $resolved streams resolved with an itag"
songs=$(grep -oE 'now playing [A-Za-z0-9_-]{11}' "$log" | sort -u | wc -l)
[ "$songs" -ge 3 ] && pass "$songs different songs played" ||
    fail "only $songs different songs played"
grep -q 'account: signed out' "$log" && pass "signed out" ||
    fail "the account check didn't report signed out"
# The app's own errors and panics (a cover that 404s is gpui's, and fine).
errors='panicked|ERROR (ytfast|ytfast_gpui)[] :]'
if grep -qE "$errors" "$log" || [ -s "$state/cache/ytfast/panics-gpui.log" ]; then
    fail "errors in the log: $(grep -E "$errors" "$log" | head -3)"
else
    pass "no errors or panics in the log"
fi
cp "$log" artifacts/gpui/smoke.log

"$input" stop
pause 1
# Audio plays in the app's process, so it ends with it.
if app_running; then fail "the app still runs after quitting"; else pass "the app (and its audio) quit"; fi

echo "== $shots captures, $failures failed checks, $(($(date +%s) - started)) s"
[ "$failures" -eq 0 ]
