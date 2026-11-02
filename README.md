<p align="center">
  <img src="macos/Assets.xcassets/Angelly.appiconset/icon_256.png" alt="Angelly icon" width="128">
</p>

<h1 align="center">Angelly</h1>

<p align="center">
  Ghostty, with a little extra taste.
  <br />
  A personal macOS fork of <a href="https://ghostty.org">Ghostty</a>.
</p>

Angelly is a fast, native terminal for macOS, built on Ghostty. It keeps the
upstream terminal foundation and adds a few personal touches to the macOS app.

## Screenshots

![Angelly screenshot](assets/angelly.webp)

## Angelly features

- ⚙️ Settings organized into clear sections.
- 🔄 Automatic update checks and downloads.
- ↔️ Resizable tab sidebar with remembered width.
- 🗂️ Tab groups with color coding.
- ➕ Choose where new tabs open.
- ♻️ Recover recently closed windows, including after restarting Angelly.

Angelly is a personal, vibe-coded project and may have rough edges.

## Recovering closed windows

With recovery enabled, the red window button saves the whole window's layout
before closing it.

Use **File > Reopen Closed Window**, or choose an older window from
**File > Recently Closed Windows**. Closing a new window after your working
window keeps both in history. The history retains up to 20 windows within a
10 MiB limit and survives restarting Angelly, even after Undo expires.

Recovery opens new terminal sessions with the saved tabs, splits, titles,
groups, and available working directories. It cannot resume terminated
processes or restore scrollback. Individual tabs closed inside a surviving
window still use the existing temporary Undo.

The default `window-save-state = always` enables recovery. `never` disables
it and clears saved history; `default` follows the macOS window preservation
preference. Use **Clear Recently Closed Windows** to delete saved history.
Custom-command terminals and the quick terminal are excluded. If a window
cannot be saved, Angelly keeps it open unless you choose **Close Without
Recovery**. Cmd-Q continues to preserve windows left open through the usual
macOS restoration behavior.

## Project direction

There’s no set roadmap. Angelly started as a way to add sidebar tabs and tab
groups to Ghostty. Have an idea? You’re welcome to contribute or [open an
issue](https://github.com/adetxt/angelly/issues).

Angelly is currently macOS-only. Linux and GTK code remains in the repository,
but this fork's Linux app is not currently maintained or tested. Release builds
are signed with a Developer ID and notarized by Apple.

## Install

Download the DMG from [Releases](https://github.com/adetxt/angelly/releases)
and drag Angelly into Applications.

## Build

You need macOS, Xcode, and [Zig](https://ziglang.org/download/) 0.17.0. Install
the Metal toolchain with:

```sh
xcodebuild -downloadComponent MetalToolchain
```

```sh
zig build -Doptimize=ReleaseFast
open zig-out/Angelly.app
```

Run targeted tests with, for example, `zig build test -Dtest-filter="adjust"`.
See [HACKING.md](HACKING.md) for general development notes inherited from
Ghostty.

## Credits and license

Angelly is built on [Ghostty](https://github.com/ghostty-org/ghostty) by
Mitchell Hashimoto and the Ghostty contributors. This is an independent fork,
not affiliated with or endorsed by Ghostty or its maintainers. Ghostty and
Angelly are released under the MIT license. See [LICENSE](LICENSE).
