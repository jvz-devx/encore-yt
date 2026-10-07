#!/usr/bin/env bash
# Submits a release to winget (microsoft/winget-pkgs) by hand, with komac,
# as the GitHub account `gh` is signed in to, through that account's fork
# of winget-pkgs. See packaging/winget/README.md.
#
#   scripts/winget-submit.sh 0.1.0-alpha.2            # dry run: prints the manifests
#   scripts/winget-submit.sh 0.1.0-alpha.2 --submit   # opens the pull request
#
# The token comes from `gh auth token` when the script runs and only ever
# sits in komac's environment: it isn't stored, printed or passed on the
# command line.

set -euo pipefail

PACKAGE="jvz-devx.encore-yt"
REPO="jvz-devx/encore-yt"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
COMMITTED="$HERE/packaging/winget"

usage() {
	echo "usage: winget-submit.sh VERSION [--submit]" >&2
	exit 2
}

version="${1:-}"
[[ -n "$version" ]] || usage
version="${version#v}"
submit=false
case "${2:-}" in
"") ;;
--submit) submit=true ;;
*) usage ;;
esac

komac() {
	if type -P komac >/dev/null; then
		command komac "$@"
	else
		nix shell nixpkgs#komac --command komac "$@"
	fi
}

GITHUB_TOKEN="$(gh auth token)"
[[ -n "$GITHUB_TOKEN" ]] || {
	echo "winget-submit: sign in with \`gh auth login\` first" >&2
	exit 1
}
export GITHUB_TOKEN

out="$(mktemp -d)"
trap 'rm -rf "$out"' EXIT

# PackageIdentifier jvz-devx.encore-yt -> manifests/j/jvz-devx/encore-yt
path="manifests/$(printf '%s' "${PACKAGE:0:1}" | tr '[:upper:]' '[:lower:]')/${PACKAGE//.//}"
status="$(curl -s -o /dev/null -w '%{http_code}' "https://api.github.com/repos/microsoft/winget-pkgs/contents/$path")"

case "$status" in
200)
	# Already in winget: a new version from the release's setup program,
	# with everything else carried over from the newest version there.
	# komac prints the manifests it writes.
	url="https://github.com/$REPO/releases/download/v$version/encore-yt-$version-windows-x86_64-setup.exe"
	komac update "$PACKAGE" --version "$version" --urls "$url" \
		--release-notes-url "https://github.com/$REPO/releases/tag/v$version" \
		--dry-run --output "$out"
	dir="$out/$path/$version"
	;;
404)
	# Not in winget yet: the first submission is the reviewed manifests in
	# the repository, which must be for this version.
	if ! grep -q "^PackageVersion: $version\$" "$COMMITTED/$PACKAGE.yaml"; then
		echo "winget-submit: $PACKAGE isn't in winget yet, and $COMMITTED is for another version" >&2
		exit 1
	fi
	dir="$out"
	cp "$COMMITTED"/"$PACKAGE"*.yaml "$dir/"
	# Parses and prints them.
	komac submit --dry-run "$dir"
	;;
*)
	echo "winget-submit: couldn't ask GitHub whether $PACKAGE is in winget (HTTP $status)" >&2
	exit 1
	;;
esac

if [[ "$submit" == true ]]; then
	komac submit --yes "$dir"
else
	printf '\nDry run. Run again with --submit to open the pull request.\n'
fi
