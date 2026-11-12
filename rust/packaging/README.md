# Packaging and platform support

## macOS

`scripts/bundle-macos.sh` builds `Angelly.app` (Info.plist, entitlements, scripting
definition, App Intents metadata, themes, shell integration, terminfo) and signs it.
`--universal` joins arm64 and x86_64 builds with `lipo`. The development bundle id is
`com.adetxt.angelly.rs`; pass `--bundle-id com.adetxt.angelly` for a release.

To build runtime resources from this checkout rather than copy them from an
installed app, run these commands from the repository root:

```sh
zig build resources -Doptimize=ReleaseFast --prefix "$PWD/zig-out"
GHOSTTY_RESOURCES_SRC="$PWD/zig-out/share" rust/scripts/bundle-macos.sh --universal --bundle-id com.adetxt.angelly
```

Tag releases use this Rust bundle, then `scripts/sign-and-notarize.sh` signs,
notarizes, and packages it. Signed Sparkle appcasts remain available for users
upgrading from the retired Swift frontend.

## Linux

`scripts/package-linux.sh` stages an FHS tree and can wrap it as a `.deb` (`--deb`,
needs `dpkg-deb`) or an AppImage (`--appimage`, needs `appimagetool`). Resources live in
`<prefix>/share/angelly`, which the executable finds relative to `bin/`.

- Desktop entry and AppStream data: `packaging/linux/`. The entry's name, icon and
  `StartupWMClass` equal `platform::APP_ID`, which the app sets as the window's app id.
  The "New Window" action runs `angelly msg new_window`.
- Control socket: `$XDG_RUNTIME_DIR/angelly/ipc.sock` (mode 0600, directory 0700), one JSON
  request per line, the same protocol AppleScript and Shortcuts use. `angelly msg '<json>'`
  or `angelly msg <cmd>` sends one request to the running instance. Set `ANGELLY_SOCKET`
  to move it; on macOS it only starts when `ANGELLY_IPC` is set.
- Not implemented: global shortcut through the `org.freedesktop.portal.GlobalShortcuts`
  portal, desktop notifications, the quick terminal panel (layer-shell), the updater's
  installer (packages update through the package manager), Dock/badge equivalents.

## Windows

Nothing platform-specific is implemented beyond cfg-gating. `cargo check` for
`x86_64-pc-windows-gnu` currently stops in `crates/terminal/src/session.rs`, which calls
the Unix-only `process_group_leader`; the platform layer has not been compiled for Windows.

## Cross-checking from macOS

```
rustup target add x86_64-unknown-linux-gnu x86_64-pc-windows-gnu
scripts/cross-check.sh linux
scripts/cross-check.sh windows
```

The Linux check passes. It type-checks only: no binary is linked or run. See the script
header for what it substitutes (zig as the C toolchain, an empty `libghostty-vt.a`, a
stub resource compiler).

CI drafts live in `.github-draft/workflows/`.
