#!/usr/bin/env bash
# Drives the desktop for visual checks of the GPUI app on KDE Plasma
# (Wayland): pointer, clicks, keys, text and screenshots, through ydotool
# and spectacle. Coordinates are screen pixels (spectacle -f captures).
#
#   scripts/gpui-input.sh setup             start ydotoold, flat pointer accel
#   scripts/gpui-input.sh move X Y          put the pointer at X,Y
#   scripts/gpui-input.sh click X Y         left click at X,Y
#   scripts/gpui-input.sh rclick X Y        right click at X,Y
#   scripts/gpui-input.sh scroll N          wheel N steps (negative is up)
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
    *) sed -n '2,16p' "$0"; exit 2 ;;
esac
