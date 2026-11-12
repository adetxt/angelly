#!/usr/bin/env bash
# Build, run and package the Linux port inside Docker, from any host with Docker.
#
# Usage: scripts/linux-docker.sh <command> [args]
#   image            build the angelly-linux-dev image (compiler, gpui libraries, Xvfb, Mesa)
#   vt               build libghostty-vt for the container's architecture with the host's zig
#   build [args]     cargo build -p angelly (pass --release for an optimized build)
#   test [args]      cargo test --workspace on Linux
#   run [name]       start a Wayland session (Weston on Xvfb) and the app, screenshot it
#                    (see packaging/linux/docker/run-wayland.sh for INPUT and EXTRA_SHOTS)
#   package          release build, then a .deb and an AppImage under target/linux-docker/
#   shell            an interactive shell in the image
#
# The cargo registry and the target directory live in the Docker volumes angelly-cargo and
# angelly-target; remove them with `docker volume rm angelly-cargo angelly-target`.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
rust=$(cd "$here/.." && pwd)
repo=$(cd "$rust/.." && pwd)
image=angelly-linux-dev
out="$rust/target/linux-docker"

case $(docker run --rm --entrypoint uname "$image" -m 2>/dev/null || echo aarch64) in
  x86_64) zig_target=x86_64-linux-gnu ;;
  *) zig_target=aarch64-linux-gnu ;;
esac

cmd=${1:-}
[ $# -gt 0 ] && shift

container() {
  mkdir -p "$out"
  docker run --rm -i ${TTY:+-t} \
    -v "$repo":/src -v angelly-cargo:/usr/local/cargo/registry -v angelly-target:/target \
    -v "$out":/out \
    -e GHOSTTY_VT_PREFIX=/out/vt -e HOME=/tmp/home \
    ${INPUT:+-e INPUT} ${EXTRA_SHOTS:+-e EXTRA_SHOTS} ${DELAY:+-e DELAY} \
    "$image" "$@"
}

case $cmd in
  image)
    docker build -t "$image" -f "$rust/packaging/linux/docker/Dockerfile" "$rust/packaging/linux/docker"
    ;;
  vt)
    (cd "$repo" && zig build -Demit-lib-vt -Doptimize=ReleaseFast -Dtarget="$zig_target" --prefix "$out/vt")
    ;;
  build)
    [ -f "$out/vt/lib/libghostty-vt.a" ] || "$0" vt
    container cargo build -p angelly "$@"
    ;;
  test)
    [ -f "$out/vt/lib/libghostty-vt.a" ] || "$0" vt
    container cargo test --workspace "$@"
    ;;
  run)
    [ -f "$out/vt/lib/libghostty-vt.a" ] || "$0" vt
    shot=${1:-angelly}
    container bash -c "cargo build -p angelly && SHOT=$shot exec /src/rust/packaging/linux/docker/run-wayland.sh"
    echo "screenshots: $out/$shot*.png, log: $out/$shot.log"
    ;;
  package)
    [ -f "$out/vt/lib/libghostty-vt.a" ] || "$0" vt
    container bash -c '
      set -e
      cargo build --release -p angelly
      export APPIMAGE_EXTRACT_AND_RUN=1
      scripts/package-linux.sh --binary /target/release/angelly --deb --appimage --out /target/linux/root
      cp /target/linux/*.deb /target/linux/*.AppImage /out/
      desktop-file-validate /target/linux/root/usr/share/applications/*.desktop
      appstreamcli validate --no-net /target/linux/root/usr/share/metainfo/*.metainfo.xml || true
    '
    ;;
  shell)
    TTY=1 container bash
    ;;
  *)
    sed -n '2,15p' "$0" >&2
    exit 2
    ;;
esac
