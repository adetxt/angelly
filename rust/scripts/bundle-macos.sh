#!/usr/bin/env bash
# Build Angelly.app from the Rust workspace.
#
# Usage: scripts/bundle-macos.sh [options]
#   --debug               build the dev profile instead of release
#   --universal           build arm64 and x86_64 and join them with lipo
#   --bundle-id ID        CFBundleIdentifier (default: com.adetxt.angelly.rs)
#   --version VERSION     CFBundleShortVersionString (default: from build.zig.zon)
#   --out DIR             output directory (default: target/bundle)
#   --sign IDENTITY       codesign identity; "-" signs ad hoc (default)
#   --no-build            reuse the binary already in target/
#   --binary PATH         bundle this executable instead of building one
#
# The dev bundle id differs from releases (com.adetxt.angelly) so both can be
# installed independently; pass --bundle-id com.adetxt.angelly for a release.
#
# Environment:
#   GHOSTTY_RESOURCES_SRC  a directory laid out like Contents/Resources of a built
#                          Ghostty/Angelly app; its terminfo, completions, man pages
#                          and themes are copied
#                          (default: /Applications/Angelly.app/Contents/Resources)
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
rust=$(cd "$here/.." && pwd)
repo=$(cd "$rust/.." && pwd)

profile=release
universal=0
bundle_id=com.adetxt.angelly.rs
version=$(sed -n 's/^ *\.version = "\(.*\)",/\1/p' "$repo/build.zig.zon" | head -1 | sed 's/-dev$//')
out="$rust/target/bundle"
identity=-
build=1
binary_override=

while [ $# -gt 0 ]; do
  case $1 in
    --debug) profile=debug ;;
    --universal) universal=1 ;;
    --bundle-id) bundle_id=$2; shift ;;
    --version) version=$2; shift ;;
    --out) out=$2; shift ;;
    --sign) identity=$2; shift ;;
    --no-build) build=0 ;;
    --binary) binary_override=$2; build=0; shift ;;
    -h|--help) sed -n '2,22p' "$0"; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-14.0}

cargo_profile=$profile
[ "$profile" = debug ] && cargo_profile=dev
target_dir="$rust/target"

build_for() { # triple
  (cd "$rust" && cargo build --locked --profile "$cargo_profile" -p angelly --target "$1")
}

if [ -n "$binary_override" ]; then
  binary=$binary_override
  metadata_root="$target_dir/$profile"
elif [ "$universal" = 1 ]; then
  triples=(aarch64-apple-darwin x86_64-apple-darwin)
  if [ "$build" = 1 ]; then
    for t in "${triples[@]}"; do build_for "$t"; done
  fi
  binary="$target_dir/angelly-universal-$profile"
  lipo -create -output "$binary" \
    "$target_dir/aarch64-apple-darwin/$profile/angelly" \
    "$target_dir/x86_64-apple-darwin/$profile/angelly"
  metadata_root="$target_dir/aarch64-apple-darwin/$profile"
else
  if [ "$build" = 1 ]; then
    (cd "$rust" && cargo build --locked --profile "$cargo_profile" -p angelly)
  fi
  binary="$target_dir/$profile/angelly"
  metadata_root="$target_dir/$profile"
fi
[ -x "$binary" ] || { echo "binary not found: $binary" >&2; exit 1; }

app="$out/Angelly.app"
contents="$app/Contents"
resources="$contents/Resources"
rm -rf "$app"
mkdir -p "$contents/MacOS" "$resources/icons"
cp "$binary" "$contents/MacOS/angelly"
printf 'APPL????' > "$contents/PkgInfo"

sed -e "s/@BUNDLE_ID@/$bundle_id/g" -e "s/@VERSION@/$version/g" \
  "$rust/packaging/macos/Info.plist" > "$contents/Info.plist"
plutil -lint "$contents/Info.plist" >/dev/null

cp "$rust/packaging/macos/Angelly.sdef" "$resources/Angelly.sdef"

# App icon: the .iconset layout wants 16..512 at 1x and 2x.
assets="$repo/macos/Assets.xcassets"
iconset=$(mktemp -d)/Angelly.iconset
mkdir -p "$iconset"
set_icon="$assets/Angelly.appiconset"
cp "$set_icon/icon_16.png"   "$iconset/icon_16x16.png"
cp "$set_icon/icon_32.png"   "$iconset/icon_16x16@2x.png"
cp "$set_icon/icon_32.png"   "$iconset/icon_32x32.png"
cp "$set_icon/icon_64.png"   "$iconset/icon_32x32@2x.png"
cp "$set_icon/icon_128.png"  "$iconset/icon_128x128.png"
cp "$set_icon/icon_256.png"  "$iconset/icon_128x128@2x.png"
cp "$set_icon/icon_256.png"  "$iconset/icon_256x256.png"
cp "$set_icon/icon_512.png"  "$iconset/icon_256x256@2x.png"
cp "$set_icon/icon_512.png"  "$iconset/icon_512x512.png"
cp "$set_icon/icon_1024.png" "$iconset/icon_512x512@2x.png"
iconutil --convert icns --output "$resources/Angelly.icns" "$iconset"
rm -rf "$(dirname "$iconset")"

# `macos-icon` variants, loaded by platform/app_icon.rs from Resources/icons/<name>.png.
cp "$assets/AppIconImage.imageset/macOS-AppIcon-1024px.png" "$resources/icons/official.png"
for variant in Blueprint Chalkboard Egyptian Glass Holographic Microchip Paper Retro Xray AttackOnTitan; do
  lower=$(printf '%s' "$variant" | tr '[:upper:]' '[:lower:]')
  [ "$variant" != AttackOnTitan ] || lower=attack-on-titan
  cp "$assets/Alternate Icons/${variant}Image.imageset/macOS-AppIcon-1024px.png" "$resources/icons/$lower.png"
done

# Shell integration and themes (the Angelly themes live in the repo; the full set
# comes from a built app when one is available).
mkdir -p "$resources/ghostty"
cp -R "$repo/src/shell-integration" "$resources/ghostty/shell-integration"
mkdir -p "$resources/ghostty/themes"
src=${GHOSTTY_RESOURCES_SRC:-/Applications/Angelly.app/Contents/Resources}
if [ -d "$src/ghostty/themes" ]; then
  cp -R "$src/ghostty/themes/." "$resources/ghostty/themes/"
else
  echo "note: no GHOSTTY_RESOURCES_SRC themes found; only the Angelly themes are bundled" >&2
fi
cp "$repo/resources/themes/"* "$resources/ghostty/themes/"
if [ -d "$src/terminfo" ]; then
  cp -R "$src/terminfo" "$resources/terminfo"
else
  echo "note: no terminfo found in $src; shells will use TERM=xterm-256color" >&2
fi
# Completions, editor syntax files and man pages. The app exports Resources/.. through
# XDG_DATA_DIRS and Resources/../man through MANPATH, so these only need to sit next to
# the ghostty directory.
for share in bash-completion fish zsh vim nvim bat man; do
  if [ -d "$src/$share" ]; then
    cp -R "$src/$share" "$resources/$share"
  fi
done

# App Intents metadata, produced by build.rs when the Swift shim defines intents.
if [ -d "$metadata_root/Metadata.appintents" ]; then
  cp -R "$metadata_root/Metadata.appintents" "$resources/Metadata.appintents"
fi

sign_args=(--force --deep --sign "$identity" --entitlements "$rust/packaging/macos/Angelly.entitlements")
if [ "$identity" != - ]; then
  sign_args+=(--timestamp --options runtime)
fi
codesign "${sign_args[@]}" "$app"
codesign --verify --deep --strict "$app"

echo "$app"
