#!/usr/bin/env bash
# Stage Angelly into an FHS install tree and optionally wrap it as a .deb or AppImage.
#
# Usage: scripts/package-linux.sh [options]
#   --out DIR       staging root (default: target/linux/root); the tree is DIR/usr/...
#   --version V     package version (default: from build.zig.zon)
#   --binary PATH   install this executable instead of building one
#   --deb           also write target/linux/angelly_<version>_<arch>.deb (needs dpkg-deb)
#   --appimage      also write target/linux/Angelly-<arch>.AppImage (needs appimagetool)
#
# Environment:
#   GHOSTTY_RESOURCES_SRC  a directory holding ghostty/themes and terminfo from a built
#                          Ghostty; copied when present
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
rust=$(cd "$here/.." && pwd)
repo=$(cd "$rust/.." && pwd)

out="$rust/target/linux/root"
version=$(sed -n 's/^ *\.version = "\(.*\)",/\1/p' "$repo/build.zig.zon" | head -1 | sed 's/-dev$//')
binary=
deb=0
appimage=0
id=com.adetxt.angelly

while [ $# -gt 0 ]; do
  case $1 in
    --out) out=$2; shift ;;
    --version) version=$2; shift ;;
    --binary) binary=$2; shift ;;
    --deb) deb=1 ;;
    --appimage) appimage=1 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

arch=$(uname -m)
if [ -z "$binary" ]; then
  (cd "$rust" && cargo build --release -p angelly)
  binary="$rust/target/release/angelly"
fi

# install -D is not portable, so create the parent first.
put() {
  mkdir -p "$(dirname "$3")"
  install -m "$1" "$2" "$3"
}

usr="$out/usr"
rm -rf "$out"
put 755 "$binary" "$usr/bin/angelly"
put 644 "$here/../packaging/linux/$id.desktop" "$usr/share/applications/$id.desktop"
put 644 "$here/../packaging/linux/$id.metainfo.xml" "$usr/share/metainfo/$id.metainfo.xml"

# The macOS asset catalog holds the app icon at every size hicolor wants.
for size in 16 32 64 128 256 512; do
  put 644 "$repo/macos/Assets.xcassets/Angelly.appiconset/icon_$size.png" \
    "$usr/share/icons/hicolor/${size}x${size}/apps/$id.png"
done

share="$usr/share/angelly"
mkdir -p "$share/ghostty/themes"
cp -R "$repo/src/shell-integration" "$share/ghostty/shell-integration"
cp "$repo/resources/themes/"* "$share/ghostty/themes/"
src=${GHOSTTY_RESOURCES_SRC:-}
if [ -n "$src" ] && [ -d "$src/ghostty/themes" ]; then cp -R "$src/ghostty/themes/." "$share/ghostty/themes/"; fi
if [ -n "$src" ] && [ -d "$src/terminfo" ]; then cp -R "$src/terminfo" "$share/terminfo"; fi

echo "staged $out"

if [ "$deb" = 1 ]; then
  case $arch in x86_64) debarch=amd64 ;; aarch64) debarch=arm64 ;; *) debarch=$arch ;; esac
  mkdir -p "$out/DEBIAN"
  cat > "$out/DEBIAN/control" <<CONTROL
Package: angelly
Version: $version
Section: x11
Priority: optional
Architecture: $debarch
Maintainer: Angelly
Depends: libc6, libfontconfig1, libxkbcommon0, libwayland-client0, libvulkan1
Description: A fast, native terminal emulator
CONTROL
  dpkg-deb --root-owner-group --build "$out" "$rust/target/linux/angelly_${version}_${debarch}.deb"
  rm -rf "$out/DEBIAN"
fi

if [ "$appimage" = 1 ]; then
  appdir="$rust/target/linux/Angelly.AppDir"
  rm -rf "$appdir"
  cp -R "$out" "$appdir"
  ln -s "usr/share/applications/$id.desktop" "$appdir/$id.desktop"
  ln -s "usr/share/icons/hicolor/256x256/apps/$id.png" "$appdir/$id.png"
  ln -s usr/bin/angelly "$appdir/AppRun"
  ARCH=$arch appimagetool "$appdir" "$rust/target/linux/Angelly-$arch.AppImage"
fi
