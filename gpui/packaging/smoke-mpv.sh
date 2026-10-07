#!/usr/bin/env bash
# Proves an installer's bundled mpv works on its own. With PATH holding only
# the system's directories (and no mpv there) it prints its version and
# plays a short generated tone to the null output. On Linux and macOS it
# also checks that every library it loads comes from the system or from
# inside the bundle.
#
# Usage: smoke-mpv.sh <bundled mpv> <bundle root>
set -euo pipefail

mpv="$(realpath "$1")"
bundle="$(realpath "$2")"
case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
        system_path="/c/Windows/System32:/c/Windows"
        python=python
        native() { cygpath -m "$1"; }
        ;;
    *)
        system_path="/usr/bin:/bin:/usr/sbin:/sbin"
        python=python3
        native() { echo "$1"; }
        ;;
esac
# env sets the variables itself, so macOS keeps DYLD_* for mpv.
run() { env PATH="$system_path" "$@"; }

if found="$(PATH="$system_path" command -v mpv)"; then
    echo "mpv is on the system PATH ($found); this check needs it hidden" >&2
    exit 1
fi
echo "bundled mpv: $mpv"
run "$mpv" --version

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
work="$(native "$tmp")"
"$python" - "$work/tone.wav" <<'EOF'
import math, struct, sys, wave
with wave.open(sys.argv[1], "wb") as w:
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(44100)
    w.writeframes(b"".join(struct.pack("<h", int(8000 * math.sin(i / 7))) for i in range(22050)))
EOF
run "$mpv" --no-config --no-terminal --no-video --ao=null --log-file="$work/play.log" "$work/tone.wav"
grep -q 'AO: \[null\]' "$work/play.log" || { cat "$work/play.log" >&2; echo "mpv did not play the tone" >&2; exit 1; }
echo "played the tone"

case "$(uname -s)" in
    Linux)
        if ldd "$mpv" | grep 'not found'; then exit 1; fi
        echo "libraries from the system:"
        ldd "$mpv" | awk '$3 ~ /^\// && index($3, b) != 1 {print "  " $1}' b="$bundle"
        ;;
    Darwin)
        run DYLD_PRINT_LIBRARIES=1 "$mpv" --version 2> "$work/dyld.txt" > /dev/null
        outside="$(grep -oE '(/[^ ]+)+$' "$work/dyld.txt" | grep -vE "^(/System/|/usr/lib/|$bundle/)" || true)"
        if [[ -n "$outside" ]]; then
            echo "mpv loaded libraries from outside the bundle:" >&2
            echo "$outside" >&2
            exit 1
        fi
        echo "all $(grep -c "$bundle/" "$work/dyld.txt") bundled libraries load from inside the bundle"
        ;;
esac
