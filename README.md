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
- Create, open, and safely remove Git worktrees from either tab layout.

Angelly is a personal, vibe-coded project and may have rough edges.

## Git worktrees

Use **Settings > Worktrees** to choose a parent folder (default: `~/worktrees`).
New worktrees are created at `<parent>/<repository>/<folder>`; changing the
setting leaves existing worktrees in place.

Use the **+** menu at the top of the sidebar (or at the end of horizontal tabs)
to open a **New Tab** or **Create Worktree…**. The worktree sheet manages the
active terminal's repository; you can also choose another repository there.
Create a new branch from the selected checkout's HEAD, or select an existing local
branch that isn't already checked out. **Create and Open** starts a new terminal
tab in the worktree; **Open Tab** opens an existing checkout.

Removal keeps the branch and never uses force. Main and locked checkouts are
protected, removal refuses worktrees with uncommitted, untracked, or ignored files, and
Angelly asks you to close or move terminals whose reported working directory
is inside the worktree. Requires Git from the Xcode Command Line Tools.

Git tabs use a single line with an icon and title. The icon is gray by default,
green for an open PR, white for a draft PR (adaptive in light mode), purple for
a merged PR, and red for merge conflicts reported by GitHub or unresolved
conflicts in the local checkout. Hover a tab for checkout, branch, working-tree, upstream,
and PR details. Ahead/behind counts use local tracking refs without fetching.
Metadata refreshes in the background while Angelly is active and follows
directory and split-focus changes.

GitHub PR details include the PR number and Open, Draft, Merged, Closed, or Conflict state.
This uses an optional installed [GitHub CLI](https://cli.github.com/), authenticated
with `gh auth login`, and refreshes about once a minute. **No PR** means GitHub
CLI found no PR for the branch; **PR unavailable** means lookup failed or the
CLI/authentication is unavailable. Detached checkouts and repositories without
remotes do not query PR status.

The **Transparent** titlebar style uses a full-size layout with the window title
hidden and the traffic lights visible. The sidebar and horizontal tab bar reserve
space for the window controls. Choose the titlebar style in **Settings > Window**;
style changes apply to new windows. The branch remains available in tab details
and native window metadata.

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
