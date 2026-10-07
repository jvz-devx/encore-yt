#!/usr/bin/env bash
# Drives the desktop for visual checks of the GPUI app on KDE Plasma
# (Wayland): pointer, clicks, keys, text and screenshots, through ydotool
# and spectacle. Coordinates are screen pixels (spectacle -f captures).
#
#   scripts/gpui-input.sh setup             start ydotoold, flat pointer accel
#   scripts/gpui-input.sh launch [BIN]      stop any running copy, start BIN
#                                           (default gpui/target/debug/ytfast-gpui,
#                                           log in artifacts/gpui/run.log) and
#                                           place its window at 0,0 1280x1000
#   scripts/gpui-input.sh stop              stop the app and the mpv it started
#   scripts/gpui-input.sh locked CMD...     run CMD holding the desktop lock, so
#                                           only one agent drives the screen
#   scripts/gpui-input.sh move X Y          put the pointer at X,Y
#   scripts/gpui-input.sh click X Y         left click at X,Y
#   scripts/gpui-input.sh rclick X Y        right click at X,Y
#   scripts/gpui-input.sh scroll N          wheel N steps (on KWin negative scrolls down)
#   scripts/gpui-input.sh key KEYS...       ydotool key codes, e.g. 56:1 62:1 62:0 56:0 (Alt+F4)
#   scripts/gpui-input.sh type TEXT         type text into the focused field
#   scripts/gpui-input.sh shot NAME [full]  capture the active window (or the
#                                           screen) to artifacts/gpui/NAME.png
#
# ydotool's absolute move only reaches 0,0 on KWin, so a move is a reset to
# the corner and then a relative move, with the virtual device set to a flat,
# zero acceleration profile so relative moves are 1:1.
set -euo pipefail
cd "$(dirname "$0")/.."

export WAYLAND_DISPLAY="${WAYLAND_DISPLAY:-wayland-0}"
export XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}"
export YDOTOOL_SOCKET="$XDG_RUNTIME_DIR/.ydotool_socket"

qdbus() { qdbus-qt6 "$@"; }

ydotool_device() {
    local d
    for d in $(qdbus org.kde.KWin /org/kde/KWin/InputDevice \
        org.kde.KWin.InputDeviceManager.ListPointers); do
        if [ "$(qdbus org.kde.KWin "/org/kde/KWin/InputDevice/$d" \
            org.freedesktop.DBus.Properties.Get org.kde.KWin.InputDevice name)" \
            = "ydotoold virtual device" ]; then
            echo "/org/kde/KWin/InputDevice/$d"
            return
        fi
    done
}

setup() {
    if [ ! -S "$YDOTOOL_SOCKET" ]; then
        sudo -n setsid ydotoold --socket-path="$YDOTOOL_SOCKET" \
            --socket-own="$(id -u):$(id -g)" >/dev/null 2>&1 </dev/null &
        for _ in $(seq 20); do [ -S "$YDOTOOL_SOCKET" ] && break; sleep 0.2; done
    fi
    local dev=""
    for _ in $(seq 20); do dev="$(ydotool_device)"; [ -n "$dev" ] && break; sleep 0.2; done
    [ -n "$dev" ] || { echo "no ydotoold device in KWin" >&2; exit 1; }
    qdbus org.kde.KWin "$dev" org.freedesktop.DBus.Properties.Set \
        org.kde.KWin.InputDevice pointerAccelerationProfileFlat true
    qdbus org.kde.KWin "$dev" org.freedesktop.DBus.Properties.Set \
        org.kde.KWin.InputDevice pointerAcceleration 0.0
    # A monitor in DPMS off gets no frame callbacks, so nothing redraws.
    kscreen-doctor --dpms on >/dev/null 2>&1 || true
}

LOCK="$XDG_RUNTIME_DIR/ytfast-gpui-desktop.lock"
# The window's frame on screen after `launch`: x, y, width, height.
GEOMETRY="0 0 1280 1000"

place() {
    local script
    script="$(mktemp --suffix=.js)"
    read -r x y w h <<<"$GEOMETRY"
    cat >"$script" <<JS
for (const w of workspace.windowList()) {
    if (w.resourceClass === "ytfast-gpui") {
        w.frameGeometry = { x: $x, y: $y, width: $w, height: $h };
        workspace.activeWindow = w;
    }
}
JS
    qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript ytfast-place >/dev/null 2>&1 || true
    qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$script" ytfast-place >/dev/null
    qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.start >/dev/null
    sleep 0.5
    qdbus org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript ytfast-place >/dev/null
    rm -f "$script"
}

# PIDs of every running copy of the app, whatever its file name or path:
# matched on the executable's own name (/proc/PID/exe), never on command
# lines, so a shell that merely mentions the app is never hit.
app_pids() {
    local pid exe
    for pid in /proc/[0-9]*; do
        exe="$(readlink "$pid/exe" 2>/dev/null)" || continue
        case "${exe##*/}" in ytfast-gpui*) echo "${pid#/proc/}" ;; esac
    done
}

stop() {
    local pids
    pids="$(app_pids)"
    [ -n "$pids" ] && kill $pids 2>/dev/null || true
    for _ in $(seq 20); do [ -z "$(app_pids)" ] && break; sleep 0.2; done
    # The backend's mpv names itself "ytfast" to PulseAudio/PipeWire. Match
    # only processes called mpv: `pkill -f` would also hit any shell whose
    # command line happens to contain the pattern.
    local pid
    for pid in $(pgrep -x mpv); do
        if tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null | grep -q 'audio-client-name=ytfast'; then
            kill "$pid" 2>/dev/null || true
        fi
    done
}

launch() {
    local bin="${1:-gpui/target/debug/ytfast-gpui}"
    stop
    mkdir -p artifacts/gpui
    setsid "$bin" >artifacts/gpui/run.log 2>&1 </dev/null &
    # Wait for the window to map, then put it where checks expect it.
    sleep 4
    place
    sleep 1
}

move() {
    ydotool mousemove -a -x 0 -y 0
    sleep 0.15
    ydotool mousemove -x "$1" -y "$2"
    sleep 0.2
}

cmd="${1:-}"
shift || true
case "$cmd" in
    setup) setup ;;
    launch) launch "$@" ;;
    stop) stop ;;
    place) place ;;
    # -o: the app a command launches must not inherit (and keep) the lock.
    locked) exec flock -o "$LOCK" "$@" ;;
    move) move "$1" "$2" ;;
    click) move "$1" "$2"; ydotool click 0xC0 ;;
    rclick) move "$1" "$2"; ydotool click 0xC1 ;;
    scroll) ydotool mousemove -w -x 0 -y "$1" ;;
    key) ydotool key "$@" ;;
    type) ydotool type -- "$*" ;;
    shot)
        mkdir -p artifacts/gpui
        mode=-a
        [ "${2:-}" = full ] && mode=-f
        spectacle -b -n "$mode" -o "artifacts/gpui/$1.png"
        echo "artifacts/gpui/$1.png"
        ;;
    *) sed -n '2,24p' "$0"; exit 2 ;;
esac
