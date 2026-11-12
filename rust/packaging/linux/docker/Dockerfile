# Build and run environment for the Linux port: compiler, the libraries gpui links, a
# software Vulkan driver and an X server, so the app can be built, screenshotted and
# packaged without a Linux machine. The source tree is mounted, not copied.
#
#   docker build -t angelly-linux-dev -f packaging/linux/docker/Dockerfile packaging/linux/docker
#   scripts/linux-docker.sh build|run|package|shell
FROM rust:1-bookworm

RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        clang libclang-dev cmake pkg-config git curl ca-certificates xz-utils file \
        libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev \
        libwayland-dev libxcb1-dev libxcb-xkb-dev libx11-xcb-dev libvulkan-dev libssl-dev \
        mesa-vulkan-drivers libgl1-mesa-dri libegl1 \
        xvfb x11-utils xdotool imagemagick dbus dbus-x11 \
        fonts-dejavu-core fonts-noto-color-emoji \
        desktop-file-utils appstream dpkg-dev fakeroot squashfs-tools zsync \
    && rm -rf /var/lib/apt/lists/*

# Weston on top of Xvfb gives a Wayland session with software rendering. gpui's X11 backend
# under plain Xvfb presents black frames, so screenshots go through Wayland.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        weston vulkan-tools \
    && rm -rf /var/lib/apt/lists/*

# appimagetool is itself an AppImage; extract it so no FUSE is needed.
RUN arch=$(uname -m) \
    && curl -fsSL -o /tmp/appimagetool "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${arch}.AppImage" \
    && chmod +x /tmp/appimagetool && cd /opt && /tmp/appimagetool --appimage-extract >/dev/null \
    && mv squashfs-root appimagetool && ln -s /opt/appimagetool/AppRun /usr/local/bin/appimagetool \
    && rm /tmp/appimagetool

ENV CARGO_TARGET_DIR=/target \
    XDG_RUNTIME_DIR=/tmp/runtime \
    WGPU_BACKEND=vulkan
WORKDIR /src/rust
