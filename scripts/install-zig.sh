#!/usr/bin/env bash
# Download a Zig release for macOS, verify it against the Zig Software
# Foundation's minisign key, and unpack it. Community mirrors are tried in
# random order before ziglang.org itself, as https://ziglang.org/download/
# asks of CI.
#
# Usage: scripts/install-zig.sh <version> <dest-dir>
# Requires curl and minisign. Prints the directory containing the zig binary.
set -euo pipefail

version="${1:?usage: install-zig.sh <version> <dest-dir>}"
dest="${2:?usage: install-zig.sh <version> <dest-dir>}"

# From https://ziglang.org/download/
key="RWSGOq2NVecA2UPNdBUZykf1CCb147pkmdtYxgb3Ti+JO/wCYvhbAb/U"

case "$(uname -m)" in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
name="zig-$arch-macos-$version"
tarball="$name.tar.xz"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Download the tarball and its signature from one base URL, and check that the
# signature is valid and its trusted comment names this exact tarball.
fetch() {
  curl -fsSL --retry 2 -o "$work/$tarball" "$1/$tarball?source=angelly-ci" &&
    curl -fsSL --retry 2 -o "$work/$tarball.minisig" "$1/$tarball.minisig?source=angelly-ci" &&
    minisign -V -P "$key" -m "$work/$tarball" -x "$work/$tarball.minisig" |
    grep -Eq "^Trusted comment: timestamp:[0-9]+[[:space:]]+file:${tarball//./\\.}[[:space:]]+hashed$"
}

mirrors=$(curl -fsSL --retry 2 https://ziglang.org/download/community-mirrors.txt | sort -R || true)
ok=""
for base in $mirrors "https://ziglang.org/download/$version"; do
  if fetch "$base"; then
    ok=1
    break
  fi
  echo "could not get a verified $tarball from $base" >&2
done
[ -n "$ok" ] || { echo "no source served a verified $tarball" >&2; exit 1; }

mkdir -p "$dest"
tar -xJf "$work/$tarball" -C "$dest"
echo "$dest/$name"
