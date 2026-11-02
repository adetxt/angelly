# Closed Window Recovery Design

Status: accepted and implemented on 2026-11-02. See the implementation plan's execution record for verification results.

## Intent

Make a terminal window recoverable after someone closes it with the red button and misses Undo. In particular, closing a window with existing tabs and then a new single-terminal window must preserve recovery choices for both, including after Angelly quits and relaunches.

## Current behavior and ownership

- `TerminalController.windowShouldClose` delegates AppKit close events to `TabGroupCloseCoordinator`, which distinguishes closing one tab from closing the whole native tab group. A visible window can contain several AppKit windows representing its tabs.
- `TerminalController.closeWindowImmediately` registers live Undo, clears the member controllers' surface trees, and closes the windows. Capture must happen before those trees are cleared.
- `BaseTerminalController.undoExpiration` reads `undo-timeout`, whose default is five seconds. Undo retains live surface trees; it is different from disk restoration.
- `TerminalRestorableState` already serializes supported terminal layout and metadata. `Ghostty.SurfaceView.init(from:)` creates terminal surfaces while decoding. Loading a history menu must never decode these objects.
- `AppDelegate` owns app-wide Undo, startup/reopen behavior, menu validation, and `NSQuitAlwaysKeepsWindows`. AppKit continues to own restoration of windows left open at quit.

## Approaches considered

1. Increase Undo's timeout or add a close warning. Cheap, but expired Undo and app restarts still lose the recovery path.
2. Automatically resurrect the last closed workspace. Convenient for accidental closes, but also reopens deliberately closed windows and introduces competing startup restoration rules.
3. Persist a bounded recently closed window history. Recommended: recovery survives Undo expiration and restart, and the user chooses which closed window to reopen.

Approach 3 was approved for implementation.

## User behavior

- Add **File > Reopen Closed Window** for the most recent recoverable window.
- Add **File > Recently Closed Windows**, newest first, with a title preview, tab count, and close time so the window with existing tabs can be distinguished from a new single-terminal window.
- Include **Clear Recently Closed Windows**. An empty history shows a disabled **No Recently Closed Windows** item.
- These commands work even when no terminal windows remain. Use native menu keyboard navigation; add no shortcut until existing configurable bindings have been checked.
- A red-button whole-window close records one history entry containing all its tabs. An individual tab close inside a surviving window continues to use existing Undo; persistent individual-tab history is outside this first version. Closing the final tab is a window close and is recorded.
- Retain ordinary single-terminal windows too. Do not guess that a shell is disposable or empty. Closing the newer window never replaces the older entry.
- Keep existing startup and Dock reopen behavior. Closed windows reopen only through an explicit recovery command. Cmd-Q continues to restore windows that were open at quit through AppKit.
- Recovery recreates supported layouts using new shell sessions. It does not restore terminated processes, previous command execution, or scrollback. Menu help should say: **Reopens the layout in new terminal sessions.**
- Keep existing running-process confirmations. Do not add a routine warning to every successful close.

## Snapshot and restore

Store a versioned window envelope with a UUID, close timestamp, short display metadata, window frame, selected tab index, and ordered serialized tab states. Each tab includes the `TerminalRestorableState` version alongside its encoded data. Capture bytes while the original surfaces exist; retain no surface/controller references in the persistent history.

Reuse the existing terminal state codec and restoration presentation behavior. Factor the shared state-application logic out of `TerminalWindowRestoration.restoreWindow` only as needed for the explicit recovery entry point. Do not call AppKit's launch-restoration callback as a general reopening API.

Decode terminal states only after an explicit user recovery request. Create fresh surface IDs and fresh sidebar group IDs, preserving references within the recovered window, so another live or recovered window cannot share its identities. Keep normal AppKit restore identity behavior unchanged. A small decoder context in `SurfaceView_AppKit.swift` can supply replacement IDs for recovery only; remap focused-surface references with the same context.

Rebuild native tab order explicitly, separating any automatic native grouping before joining the recovered tabs. Restore titles, tab colors, group metadata/collapse state, splits, selected tab, focus, fullscreen style, and a screen-constrained frame using existing behavior. Missing working directories fall back to the ordinary configured startup directory with an explanation after recovery.

Respect existing non-restorable custom-command terminals and exclude quick terminal windows. Do not silently call a partial snapshot a complete window: if any member cannot be captured, treat recovery capture as unavailable and offer the close decision described below.

After successful terminal construction, consume that history entry. If terminal construction fails, keep it for retry and clean up any partially created terminals without creating new recovery entries. Loading/listing a history file must create zero terminal processes.

History cleanup failure is separate from terminal construction failure. If deleting the entry fails after persistent recovery or live Undo succeeds, keep the reopened terminals, mark the runtime ticket restored, hide the stale history item, and show a warning with Retry. State plainly that the old history entry may reappear after restart until cleanup succeeds. Do not kill restored terminals to roll back a history write or promise durable exactly-once recovery during a disk failure.

## Undo and close lifecycle

- Capture after process confirmation succeeds and immediately before destructive close. Canceling confirmation creates no entry.
- A whole tab group produces exactly one record. Close All Windows records each logical window once, even though `TerminalController.all` includes tab controllers.
- A normal app quit does not add duplicates to history. Redoing a close creates a new valid close record; undoing creation is not a user window-close record. Trace callers explicitly rather than using disabled Undo registration as a proxy for user intent.
- Each recorded close has a small runtime ticket shared with its live Undo closure. Successful Undo consumes the matching disk entry. Successful persistent recovery marks the ticket restored so a later stale Undo cannot reopen the same close again. Pruning a disk entry must not disable otherwise valid live Undo.
- Persist before destroying the window. If encoding or writing fails, retain the open window and show **Couldn’t Save Window for Recovery**, with **Cancel** as the default and **Close Without Recovery** as the explicit alternative. Capture again after a sheet yields to the event loop; never reuse stale tab membership. Failure paths must retain a functioning Undo path when the user elects to close.

## Storage and privacy

- macOS only; no new dependencies or changes to Zig configuration defaults.
- Use `Application Support/<bundle identifier>/recently-closed-windows.json`, isolated between debug and release bundle identifiers.
- Store at most 20 windows and at most 10 MiB of encoded history, evicting oldest entries first. A single oversized window fails capture rather than discarding existing history.
- Use atomic file replacement and owner-only directory/file permissions. Serialize mutations through the app-owned store on the main actor; perform the bounded close-time save before close, including when last-window auto-quit is enabled. Do not leave a detached pending save at shutdown.
- The effective `NSQuitAlwaysKeepsWindows` preference controls disk recovery, matching existing `window-save-state` behavior. `always` enables it; `never` disables loading/capture and clears existing recovery history; `default` follows the system preference. A change to an effective disabled state clears loaded and on-disk history. Report deletion failure and never expose stale entries while disabled.
- When persistence is disabled, bypass capture and recovery-save prompts entirely. Closing and live Undo keep their existing behavior.
- Capture only the existing restorable layout fields and minimal menu metadata. Do not add environment variables, command history, scrollback, or command replay. Do not log titles, paths, or encoded contents on errors.
- Validate the outer schema, supported tab versions, byte limits, and indices before terminal decoding. Malformed/unsupported history must not launch terminals or be silently overwritten. Show recovery unavailable with a clear-history option; unrelated normal app use remains available.
- Clearing history prevents a pending operation from writing old records back. This version assumes the ordinary macOS single application process per bundle; independent processes sharing a bundle's data directory need separate coordination before support is claimed.

## Acceptance cases

1. Window A contains several tabs/groups/splits. Window B is a new single-terminal window. Close A, then B, quit, relaunch: both appear in history; selecting A restores its supported layout.
2. Repeat without quitting; let Undo expire or set `undo-timeout = 0`: persistent recovery still works.
3. Cancel a process confirmation: the window stays open and history is unchanged.
4. Undo a close: live terminals return and the matching history entry disappears. Recover via the menu first: the previous close's Undo cannot duplicate the window. If history cleanup cannot be written, terminals remain open, the stale entry stays hidden in this run, and Retry can remove it; the warning explains restart may expose the old entry again.
5. Cmd-Q with windows open: AppKit restores them once; history does not add another copy. Last-window auto-quit still saves a red-button close before termination.
6. Disk write failure: the default action keeps the original window open; explicit Close Without Recovery closes with the existing Undo behavior available.
7. A restart, corrupt file, incompatible version, missing directory, unavailable display, history eviction, or config disable does not produce silent data loss or unexpected terminal launches. With persistence disabled, a red-button close never attempts capture or shows a recovery-save prompt, including after a failed attempt to delete old history.

## Scope and remaining decisions

The history limit, menu location, manual reopening, and configuration coupling above are the implemented defaults. This change does not provide crash checkpoints for windows that were never closed, named workspaces, persistent individual-tab history, or background process survival. A general live-workspace autosave system would be a separate project.
