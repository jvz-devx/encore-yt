#!/usr/bin/env bash
# Points Casks/encore-yt.rb at a release: its version and the SHA-256 of
# both disk images, taken from the release's checksums.txt. The release
# workflow runs it after publishing a release and commits the result.
#
#   scripts/update-cask.sh v0.1.0-alpha.2 path/to/checksums.txt

set -euo pipefail

tag="${1:?usage: update-cask.sh TAG CHECKSUMS}"
sums="${2:?usage: update-cask.sh TAG CHECKSUMS}"
cask="$(dirname "$0")/../Casks/encore-yt.rb"
version="${tag#v}"

sha_of() {
	local file="encore-yt-$version-macos-$1.dmg" sum
	sum="$(awk -v file="$file" '$2 == file || $2 == "*" file { print $1; exit }' "$sums")"
	if [[ ! "$sum" =~ ^[0-9a-f]{64}$ ]]; then
		echo "update-cask: no SHA-256 for $file in $sums" >&2
		exit 1
	fi
	echo "$sum"
}

arm="$(sha_of arm64)"
intel="$(sha_of x86_64)"

sed -i.bak \
	-e "s/^\(  version \"\)[^\"]*\"/\1$version\"/" \
	-e "s/\(arm: *\"\)[0-9a-f]\{64\}\"/\1$arm\"/" \
	-e "s/\(intel: *\"\)[0-9a-f]\{64\}\"/\1$intel\"/" \
	"$cask"
rm -f "$cask.bak"

if ! grep -q "version \"$version\"" "$cask" || ! grep -q "$arm" "$cask" || ! grep -q "$intel" "$cask"; then
	echo "update-cask: $cask didn't take the new values" >&2
	exit 1
fi
echo "Cask: $version, arm64 $arm, x86_64 $intel"
