#!/usr/bin/env bash
# Vendors a yt-dlp-ejs release (github.com/yt-dlp/ejs) into crates/core/src/jsc/ and pins
# its hashes in crates/core/src/jsc/pins.txt, if it is newer than the vendored one.
#   scripts/ejs-bump.sh            # the latest release
#   scripts/ejs-bump.sh 0.9.0      # a given release
#   EJS_FORCE=1 scripts/ejs-bump.sh 0.8.0   # re-vendor even if not newer
# Each file must match the SHA-256 digest GitHub records for the release
# asset and still carry the Unlicense header. Prints what it did; in GitHub
# Actions it also writes bumped/version/previous to $GITHUB_OUTPUT.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
jsc="$root/crates/core/src/jsc"
pins="$jsc/pins.txt"
api=https://api.github.com/repos/yt-dlp/ejs/releases
auth=()
[[ -n "${GH_TOKEN:-}" ]] && auth=(-H "Authorization: Bearer $GH_TOKEN")

output() { [[ -n "${GITHUB_OUTPUT:-}" ]] && echo "$1=$2" >> "$GITHUB_OUTPUT"; return 0; }

# The release the vendored files are pinned to (the newest if several).
vendored() {
  local lib core
  lib=$(sha256sum "$jsc/ejs-lib.min.js" | cut -d' ' -f1)
  core=$(sha256sum "$jsc/ejs-core.min.js" | cut -d' ' -f1)
  comm -12 <(awk -v h="$lib" '$1 == h && $3 == "yt.solver.lib.min.js" { print $2 }' "$pins" | sort) \
           <(awk -v h="$core" '$1 == h && $3 == "yt.solver.core.min.js" { print $2 }' "$pins" | sort) \
    | sort -V | tail -1
}

newer() { [[ "$1" != "$2" && "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" == "$1" ]]; }

previous=$(vendored)
[[ -n "$previous" ]] || { echo "the vendored EJS files aren't pinned in $pins" >&2; exit 1; }
if [[ -n "${1:-}" ]]; then
  release=$(curl -fsSL "${auth[@]}" "$api/tags/$1")
else
  release=$(curl -fsSL "${auth[@]}" "$api/latest")
fi
version=$(jq -r .tag_name <<<"$release")
output previous "$previous"
output version "$version"
echo "vendored EJS $previous, release $version"
if ! newer "$version" "$previous" && [[ -z "${EJS_FORCE:-}" ]]; then
  echo "up to date"
  output bumped false
  exit 0
fi

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
for asset in yt.solver.lib.min.js yt.solver.core.min.js; do
  url=$(jq -r --arg n "$asset" '.assets[] | select(.name == $n) | .browser_download_url' <<<"$release")
  digest=$(jq -r --arg n "$asset" '.assets[] | select(.name == $n) | .digest' <<<"$release")
  [[ -n "$url" && "$digest" == sha256:* ]] || { echo "$version has no $asset with a digest" >&2; exit 1; }
  curl -fsSL -o "$scratch/$asset" "$url"
  hash=$(sha256sum "$scratch/$asset" | cut -d' ' -f1)
  [[ "sha256:$hash" == "$digest" ]] || { echo "$asset: $hash doesn't match $digest" >&2; exit 1; }
  head -c 300 "$scratch/$asset" | grep -q 'SPDX-License-Identifier: Unlicense' \
    || { echo "$asset: no Unlicense header; check the licence before vendoring" >&2; exit 1; }
  grep -q "^$hash  $version  $asset\$" "$pins" || echo "$hash  $version  $asset" >> "$pins"
  echo "$asset $hash"
done
cp "$scratch/yt.solver.lib.min.js" "$jsc/ejs-lib.min.js"
cp "$scratch/yt.solver.core.min.js" "$jsc/ejs-core.min.js"
[[ "$(vendored)" == "$version" ]] || { echo "the pins don't name $version for the new files" >&2; exit 1; }
# ytfast-gpui 0.1.x reads the pins from the pre-rename path (src/jsc/README.md).
cp "$pins" "$(dirname "$0")/../src/jsc/pins.txt"
echo "vendored EJS $version"
output bumped true
