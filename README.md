<p align="center">
  <img src="macos/Assets.xcassets/Angelly.appiconset/icon_256.png" alt="Angelly icon" width="128">
</p>

<h1 align="center">Angelly</h1>

<p align="center">
  Ghostty, with a little extra taste.
  <br />
  A personal, vibe-coded macOS fork of <a href="https://ghostty.org">Ghostty</a>.
</p>

## Why this exists

I love [Ghostty](https://ghostty.org). It's fast, native, and well made. I just
want to add some cosmetic touches of my own on top of it, and a few small
conveniences I miss from other terminals. Angelly is where I do that.

## Heads-up: this project is vibe-coded

Angelly is built with AI assistance (Claude Code) and not reviewed like upstream
code. Expect rough edges. For a stable terminal, use [Ghostty](https://ghostty.org).

This is an independent fork. It is **not affiliated with or endorsed by**
Ghostty or its maintainers, and it isn't a contribution to upstream. Ghostty's
own [AI policy](AI_POLICY.md) is strict about AI-generated contributions, so I
keep this work in a separate project instead.

## Status

- macOS only so far. The Linux/GTK code and packaging files are still here but
  untouched and untested.
- Release builds are signed with a Developer ID and notarized by Apple.

## Install

Download the DMG from [Releases](https://github.com/adetxt/angelly/releases)
and drag Angelly into Applications.

## Build

You need macOS, Xcode (with its Metal toolchain: `xcodebuild -downloadComponent MetalToolchain`)
and [Zig](https://ziglang.org/download/) 0.17.0.

```sh
zig build -Doptimize=ReleaseFast
open zig-out/Angelly.app
```

Run targeted tests with, for example, `zig build test -Dtest-filter="adjust"`.
See [HACKING.md](HACKING.md) for general development notes inherited from
Ghostty.

## Credits and license

Angelly is built on [Ghostty](https://github.com/ghostty-org/ghostty) by
Mitchell Hashimoto and the Ghostty contributors. All the hard work is theirs,
and I'm grateful for it. Ghostty is released under the MIT license, and so is
this fork. See [LICENSE](LICENSE).
