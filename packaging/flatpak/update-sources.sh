#!/usr/bin/env bash
# Regenerates packaging/flatpak/cargo-sources.json from Cargo.lock.
# Run it whenever Cargo.lock changes, and commit the result: the Flatpak
# build has no network, so every crate is listed there with its checksum.
set -euo pipefail

# flatpak-builder-tools, pinned. Bump deliberately.
GENERATOR_REPO=https://github.com/flatpak/flatpak-builder-tools
GENERATOR_COMMIT=74697c75b630d7330e77250fc13cb5ea688d9479

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$here/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

git init -q "$work/tools"
git -C "$work/tools" fetch -q --depth 1 "$GENERATOR_REPO" "$GENERATOR_COMMIT"
git -C "$work/tools" checkout -q FETCH_HEAD

python3 -m venv "$work/venv"
"$work/venv/bin/pip" install -q aiohttp tomlkit PyYAML

"$work/venv/bin/python" "$work/tools/cargo/flatpak-cargo-generator.py" \
  "$root/Cargo.lock" -o "$here/cargo-sources.json"
echo "wrote $here/cargo-sources.json"
