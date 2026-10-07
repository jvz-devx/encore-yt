#!/bin/bash
# Installs Music (ytfast) on macOS from GitHub Releases, without Homebrew:
#
#   curl -fsSL https://raw.githubusercontent.com/jvz-devx/ytfast-gpui/main/scripts/install-macos.sh | bash
#
# It picks the build for this Mac (Apple silicon or Intel), downloads the
# newest release's disk image (pre-releases included), checks it against the
# release's checksums.txt, and copies ytfast.app to /Applications (or
# ~/Applications when /Applications isn't writable), replacing an older copy.
# The app isn't notarized; curl doesn't set the quarantine flag, and the
# script clears it anyway, so the app opens without the Gatekeeper dialog.
#
# YTFAST_VERSION=v0.1.0-alpha.1 installs that release instead of the newest.
#
# Written for the bash 3.2 that macOS ships.

set -euo pipefail

REPO="jvz-devx/ytfast-gpui"
API="https://api.github.com/repos/$REPO"
APP="ytfast.app"
BUNDLE_ID="io.github.jvz-devx.ytfast-gpui"

work=""
mount=""

say() { printf '%s\n' "$*"; }
die() {
	printf 'Music install: %s\n' "$*" >&2
	exit 1
}

cleanup() {
	if [[ -n "$mount" ]]; then
		hdiutil detach -quiet "$mount" 2>/dev/null || hdiutil detach -quiet -force "$mount" 2>/dev/null || true
	fi
	if [[ -n "$work" ]]; then
		rm -rf "$work"
	fi
}

# arm64 or x86_64. A shell under Rosetta reports x86_64 on Apple silicon;
# sysctl.proc_translated says so.
detect_arch() {
	local machine
	machine="$(uname -m)"
	if [[ "$machine" == "x86_64" && "$(sysctl -in sysctl.proc_translated 2>/dev/null || true)" == "1" ]]; then
		machine="arm64"
	fi
	case "$machine" in
	arm64 | aarch64) echo "arm64" ;;
	x86_64) echo "x86_64" ;;
	*) die "This Mac ($machine) isn't supported." ;;
	esac
}

# The macOS each build needs (as the release workflow builds them, after
# the bundled mpv): 14 on Apple silicon, 15 on Intel.
check_macos() {
	local arch="$1" need version major
	if [[ "$arch" == "arm64" ]]; then need=14; else need=15; fi
	version="$(sw_vers -productVersion)"
	major="${version%%.*}"
	if [[ "$major" -lt "$need" ]]; then
		die "Music needs macOS $need or newer on this Mac; this one has $version."
	fi
}

# The release's JSON: the pinned tag, or the newest release (the list is
# newest first and includes pre-releases).
release_json() {
	if [[ -n "${YTFAST_VERSION:-}" ]]; then
		curl -fsSL "$API/releases/tags/$YTFAST_VERSION" || die "No release $YTFAST_VERSION."
	else
		curl -fsSL "$API/releases?per_page=1" || die "Couldn't reach GitHub Releases."
	fi
}

# The first "tag_name" in the JSON on stdin. (awk reads to the end, so
# nothing upstream dies of SIGPIPE under pipefail.)
tag_name() {
	grep -o '"tag_name": *"[^"]*"' | sed 's/.*"\([^"]*\)"$/\1/' | awk 'NR == 1'
}

# The "digest" GitHub records for asset $1 in the JSON on stdin (for
# releases made before checksums.txt existed). Asset objects list "name"
# before "digest".
api_digest() {
	grep -oE '"(name|digest)": *"[^"]*"' |
		sed 's/^"\([a-z]*\)": *"\([^"]*\)"$/\1 \2/' |
		awk -v file="$1" '$1 == "name" { hit = ($2 == file) } $1 == "digest" && hit && !done { sub(/^sha256:/, "", $2); print $2; done = 1 }'
}

# The SHA-256 of $1 that the release lists: checksums.txt, else the API.
expected_sha256() {
	local file="$1" json="$2" base="$3" sum=""
	if curl -fsSL -o "$work/checksums.txt" "$base/checksums.txt" 2>/dev/null; then
		sum="$(awk -v file="$file" '$2 == file || $2 == "*" file { print $1; exit }' "$work/checksums.txt")"
	fi
	if [[ -z "$sum" ]]; then
		sum="$(printf '%s' "$json" | api_digest "$file")"
	fi
	[[ -n "$sum" ]] || die "The release lists no checksum for $file."
	echo "$sum"
}

# Where the app goes: where it already is, else /Applications if this user
# can write there, else ~/Applications.
destination() {
	if [[ -d "/Applications/$APP" && -w "/Applications" ]]; then
		echo "/Applications"
	elif [[ -d "$HOME/Applications/$APP" ]]; then
		echo "$HOME/Applications"
	elif [[ -w "/Applications" ]]; then
		echo "/Applications"
	else
		echo "$HOME/Applications"
	fi
}

# Quits a running copy, so its bundle can be replaced.
quit_running() {
	pgrep -x ytfast-gpui >/dev/null 2>&1 || return 0
	say "Quitting Music..."
	osascript -e "tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true
	local waited=0
	while [[ "$waited" -lt 10 ]]; do
		pgrep -x ytfast-gpui >/dev/null 2>&1 || return 0
		sleep 1
		waited=$((waited + 1))
	done
	pkill -x ytfast-gpui 2>/dev/null || true
	sleep 1
}

main() {
	[[ "$(uname -s)" == "Darwin" ]] || die "This script is for macOS. See https://github.com/$REPO#download"

	local arch json tag version file base expected actual dest app
	arch="$(detect_arch)"
	check_macos "$arch"

	trap cleanup EXIT
	work="$(mktemp -d "${TMPDIR:-/tmp}/ytfast-install.XXXXXX")"

	json="$(release_json)"
	tag="$(printf '%s' "$json" | tag_name)"
	[[ -n "$tag" ]] || die "Couldn't find a release."
	version="${tag#v}"
	file="ytfast-gpui-$version-macos-$arch.dmg"
	base="https://github.com/$REPO/releases/download/$tag"

	say "Downloading Music $version for $arch..."
	curl -fL --retry 3 --progress-bar -o "$work/$file" "$base/$file" || die "Couldn't download $file."

	expected="$(expected_sha256 "$file" "$json" "$base")"
	actual="$(shasum -a 256 "$work/$file" | awk '{ print $1 }')"
	if [[ "$actual" != "$expected" ]]; then
		die "The download doesn't match the release's checksum (expected $expected, got $actual)."
	fi
	say "Checksum OK."

	mkdir "$work/mnt"
	hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$work/mnt" "$work/$file" >/dev/null ||
		die "Couldn't open the disk image."
	mount="$work/mnt"
	[[ -d "$mount/$APP" ]] || die "The disk image has no $APP."

	dest="$(destination)"
	mkdir -p "$dest"
	app="$dest/$APP"
	quit_running
	rm -rf "$app.new"
	ditto "$mount/$APP" "$app.new"
	rm -rf "$app"
	mv "$app.new" "$app"
	xattr -dr com.apple.quarantine "$app" 2>/dev/null || true

	say "Installed Music $version in $app."
	say "Open it from Launchpad or Spotlight (ytfast), or run: open \"$app\""
}

main "$@"
