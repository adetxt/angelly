#!/usr/bin/env bash
# Package Angelly.app into a drag-to-Applications disk image.
#
# Usage: scripts/make-dmg.sh [path/to/Angelly.app] [output.dmg]
# Defaults: zig-out/Angelly.app -> zig-out/Angelly-<version>.dmg
set -euo pipefail

app="${1:-zig-out/Angelly.app}"
[ -d "$app" ] || { echo "app bundle not found: $app" >&2; exit 1; }

version=$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$app/Contents/Info.plist")
out="${2:-zig-out/Angelly-$version.dmg}"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

ditto "$app" "$stage/$(basename "$app")"
ln -s /Applications "$stage/Applications"

mkdir -p "$(dirname "$out")"
rm -f "$out"
hdiutil create -quiet -volname "Angelly" -srcfolder "$stage" -fs HFS+ -format UDZO "$out"

echo "$out"
