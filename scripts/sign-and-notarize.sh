#!/usr/bin/env bash
# Sign Angelly.app with a Developer ID, notarize and staple it, then package it
# into a DMG and sign, notarize and staple that too.
#
# Usage: scripts/sign-and-notarize.sh [path/to/Angelly.app] [output.dmg]
# Defaults: zig-out/Angelly.app -> zig-out/Angelly-<version>.dmg
#
# Environment:
#   SIGN_IDENTITY   codesign identity (default: the only "Developer ID
#                   Application" certificate in the keychain)
#   NOTARY_PROFILE  notarytool keychain profile (default: angelly-notary)
#   KEYCHAIN        keychain holding the identity and profile (default: search list)
#
# Prints the path of the notarized DMG.
set -euo pipefail

app="${1:-zig-out/Angelly.app}"
[ -d "$app" ] || { echo "app bundle not found: $app" >&2; exit 1; }

identity="${SIGN_IDENTITY:-Developer ID Application}"
profile="${NOTARY_PROFILE:-angelly-notary}"
root="$(cd "$(dirname "$0")/.." && pwd)"

keychain_args=()
[ -n "${KEYCHAIN:-}" ] && keychain_args=(--keychain "$KEYCHAIN")

sign() {
  codesign --force --timestamp --options runtime --sign "$identity" \
    ${keychain_args[@]+"${keychain_args[@]}"} "$@" >&2
}

# Submit a file to Apple's notary service and fail with its log unless accepted.
notarize() {
  local result id status
  result=$(xcrun notarytool submit "$1" --wait --output-format json \
    --keychain-profile "$profile" ${keychain_args[@]+"${keychain_args[@]}"})
  id=$(plutil -extract id raw - <<<"$result")
  status=$(plutil -extract status raw - <<<"$result")
  echo "notarization $id: $status" >&2
  if [ "$status" != "Accepted" ]; then
    xcrun notarytool log "$id" --keychain-profile "$profile" \
      ${keychain_args[@]+"${keychain_args[@]}"} >&2 || true
    exit 1
  fi
}

# Nested code must be signed before the bundle that contains it.
sparkle="$app/Contents/Frameworks/Sparkle.framework"
sign "$sparkle/Versions/B/XPCServices/Downloader.xpc"
sign "$sparkle/Versions/B/XPCServices/Installer.xpc"
sign "$sparkle/Versions/B/Autoupdate"
sign "$sparkle/Versions/B/Updater.app"
sign "$sparkle"
sign "$app/Contents/PlugIns/DockTilePlugin.plugin"

# The build embeds the ReleaseLocal entitlements, which include get-task-allow
# and are rejected by notarization, so sign with the release set instead.
sign --entitlements "$root/macos/Ghostty.entitlements" "$app"
codesign --verify --deep --strict "$app" >&2

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ditto -c -k --keepParent "$app" "$work/app.zip"
notarize "$work/app.zip"
xcrun stapler staple "$app" >&2

dmg=$("$root/scripts/make-dmg.sh" "$app" ${2:+"$2"})
sign "$dmg"
notarize "$dmg"
xcrun stapler staple "$dmg" >&2

spctl --assess --type execute "$app" >&2
spctl --assess --type open --context context:primary-signature "$dmg" >&2

echo "$dmg"
