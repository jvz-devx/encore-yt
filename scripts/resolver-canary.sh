#!/usr/bin/env bash
# The Rust stream resolver's canary, signed out (run daily in GitHub Actions
# by .github/workflows/resolver-canary.yml; never with cookies):
#   1. the solver: fetches the current player script and checks that the
#      embedded QuickJS solves its challenges exactly like yt-dlp's EJS in
#      deno (tests/resolver_offline.rs);
#   2. live (unless --solver-only): resolves each song with
#      YTFAST_RESOLVER=rust (examples/resolve_rust.rs) and fetches its first
#      KB, expecting 200 or 206. A song that meets YouTube's bot check
#      (datacenter IPs do) doesn't count; if every song does, the live part
#      is inconclusive and only warns.
#   scripts/resolver-canary.sh [--solver-only] [VIDEO_ID...]
# YouTube requests: 2 for the player (none with CANARY_PLAYER=<saved .js>),
# then 1 visitor id call and 2 per song. Needs deno, jq and cargo. Work files
# go to CANARY_DIR (default artifacts/canary); the resolver runs with a
# fresh, signed-out config and cache there.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
dir="${CANARY_DIR:-$root/artifacts/canary}"
live=1
songs=()
for arg in "$@"; do
  case "$arg" in
    --solver-only) live=0 ;;
    *) songs+=("$arg") ;;
  esac
done
# Two long-lived public music videos.
[[ ${#songs[@]} -gt 0 ]] || songs=(dQw4w9WgXcQ fJ9rUzIMcZQ)
agent="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36"

rm -rf "$dir"
mkdir -p "$dir/resolver" "$dir/home/config" "$dir/home/cache/ytfast/player"
failed=()

echo "== solver"
if [[ -n "${CANARY_PLAYER:-}" ]]; then
  id=$(basename "$CANARY_PLAYER" .js)
  cp "$CANARY_PLAYER" "$dir/resolver/$id.js"
else
  id=$(curl -fsSL -A "$agent" https://www.youtube.com/iframe_api \
    | grep -oE 'player\\?/[0-9a-f]{8}\\?/' | head -1 | grep -oE '[0-9a-f]{8}') \
    || { echo "FAILED: the iframe API names no player"; exit 1; }
  curl -fsSL -A "$agent" -o "$dir/resolver/$id.js" \
    "https://www.youtube.com/s/player/$id/player_ias.vflset/en_US/base.js" \
    || { echo "FAILED: couldn't download player $id"; exit 1; }
fi
echo "player $id ($(($(wc -c < "$dir/resolver/$id.js") / 1024)) KB)"
if "$root/scripts/ejs-expected.sh" "$dir/resolver/$id.js" > "$dir/resolver/$id.expected.json"; then
  echo "deno: $(jq '.n | length' "$dir/resolver/$id.expected.json") n answers, $(jq '.sig | length' "$dir/resolver/$id.expected.json") signature lengths"
  if ! (cd "$root" && YTFAST_RESOLVER_CAPTURES="$dir/resolver" \
        cargo test --no-default-features --test resolver_offline -- --nocapture); then
    failed+=("QuickJS solves player $id differently from yt-dlp's EJS (or not at all)")
  fi
else
  cat "$dir/resolver/$id.expected.json"
  failed+=("yt-dlp's EJS in deno can't solve player $id")
  rm -f "$dir/resolver/$id.expected.json"
fi

if [[ $live == 1 ]]; then
  echo "== live, signed out: ${songs[*]}"
  # The example reuses the player fetched above instead of asking again.
  cp "$dir/resolver/$id.js" "$dir/home/cache/ytfast/player/$id.js"
  printf '%s' "$id" > "$dir/home/cache/ytfast/player/current"
  (cd "$root" && cargo build --no-default-features --example resolve_rust)
  # Only the resolver gets the fresh home (cargo and sccache keep theirs).
  status=0
  env -u YTFAST_FAKE_STREAM XDG_CONFIG_HOME="$dir/home/config" \
    XDG_CACHE_HOME="$dir/home/cache" YTFAST_RESOLVER=rust \
    "$root/target/debug/examples/resolve_rust" "${songs[@]}" || status=$?
  case $status in
    0) ;;
    # YouTube's bot check for every song: GitHub's IP, not the resolver.
    3) echo "::warning::YouTube's bot check met every song from this runner; the live part is inconclusive" ;;
    *) failed+=("the Rust resolver failed signed out (see the log above)") ;;
  esac
fi

echo "== result"
if [[ ${#failed[@]} -gt 0 ]]; then
  printf 'FAILED: %s\n' "${failed[@]}"
  exit 1
fi
summary="ok: player $id solved like yt-dlp"
if [[ $live == 1 ]]; then
  if [[ $status == 3 ]]; then summary+="; live part inconclusive (bot check)"
  else summary+="; songs resolved and fetched (see above)"; fi
fi
echo "$summary"
