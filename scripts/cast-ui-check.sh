#!/usr/bin/env bash
# Launch a signed-out local-stand-in check with all app files in this worktree.
# Run through scripts/gpui-input.sh locked; subsequent desktop input must
# stay inside the same locked command while collecting evidence.
set -euo pipefail
cd "$(dirname "$0")/.."

mode="${1:-dark}"
case "$mode" in dark|light) ;; *) echo "usage: $0 [dark|light]" >&2; exit 2 ;; esac
test -f artifacts/cast/m35-silence.webm || {
    echo "Create artifacts/cast/m35-silence.webm with ffmpeg anullsrc first." >&2
    exit 2
}
mkdir -p artifacts/cast
run_dir="$(mktemp -d "$PWD/artifacts/cast/ui-$mode.XXXXXX")"
mkdir -p "$run_dir/config" "$run_dir/cache/encore-yt" "$run_dir/runtime"
chmod 700 "$run_dir/runtime"
cp crates/app/fixtures/cast-session.json "$run_dir/cache/encore-yt/session.json"

desktop_runtime="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
desktop_display="${WAYLAND_DISPLAY:-wayland-0}"
case "$desktop_display" in /*) ;; *) desktop_display="$desktop_runtime/$desktop_display" ;; esac
for socket in .ydotool_socket pipewire-0 pulse; do
    if [ -e "$desktop_runtime/$socket" ]; then
        ln -s "$desktop_runtime/$socket" "$run_dir/runtime/$socket"
    fi
done

export XDG_CONFIG_HOME="$run_dir/config"
export XDG_CACHE_HOME="$run_dir/cache"
export XDG_RUNTIME_DIR="$run_dir/runtime"
export WAYLAND_DISPLAY="$desktop_display"
export DBUS_SESSION_BUS_ADDRESS="${DBUS_SESSION_BUS_ADDRESS:-unix:path=$desktop_runtime/bus}"
export ENCORE_CAST_LOCAL_ONLY=1
if [ "${ENCORE_CAST_SHIELD_ONLY:-0}" = 1 ]; then
    export ENCORE_CAST_LOCAL_ONLY=0
fi
export ENCORE_FAKE_STREAM="$PWD/artifacts/cast/m35-silence.webm"
export ENCORE_UPDATE_FEED=http://127.0.0.1:9/releases
export ENCORE_THEME="$mode"
export ENCORE_VISUALS=0
export TMPDIR="$PWD/artifacts/cast/tmp"
scripts/gpui-input.sh launch
echo "$run_dir"
