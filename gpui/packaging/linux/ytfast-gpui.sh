#!/bin/sh
# The app lives in /usr/lib/ytfast-gpui, next to the yt-dlp and deno it
# bundles (bin/), which it puts first on PATH.
exec /usr/lib/ytfast-gpui/ytfast-gpui "$@"
