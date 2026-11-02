# Closed Window Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Preserve unrelated working-tree changes.

**Goal:** Recover a whole closed Angelly window after Undo expires or the app restarts, including when a newer single-terminal window was closed afterward.

**Architecture:** An app-owned bounded history stores serialized snapshots before terminal teardown. Reuse the existing terminal state codec and state-application behavior, with fresh identities for explicit recovery. AppKit retains ownership of normal launch restoration; the existing Undo manager retains short-lived live recovery.

**Tech Stack:** Swift, AppKit, Foundation Codable, Swift Testing, XCTest UI tests. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-11-02-closed-window-recovery-design.md`

**Status:** Implemented. The original detailed checklist below is retained as the execution guide; the execution record at the end reports completed milestones and verification limits.

## Global Constraints

- macOS only; no new dependencies or changes to Zig configuration defaults.
- Store at most 20 windows and at most 10 MiB of encoded history, evicting oldest entries first.
- When persistence is enabled, persist before destroying the window. When disabled, close normally without capture or recovery-save prompts.
- Loading/listing a history file must create zero terminal processes.
- Closed windows reopen only through an explicit recovery command.
- Respect existing non-restorable custom-command terminals and exclude quick terminal windows.
- Capture only the existing restorable layout fields and minimal menu metadata.
- Build and run unit tests through `macos/build.nu`; it skips UI tests, which require a separate local UI-enabled run.

## Review Focus

- Several AppKit tab windows represent one visible window: one close must create exactly one ordered record, including Close All Windows.
- Confirmation sheets allow tab membership/configuration to change: capture current membership only after confirmation, and recheck after recovery-save failure prompts.
- Live Undo and disk recovery refer to one close: either can consume it once, without history pruning disabling live Undo.
- State decoding starts shells: listing, validating, disabling, and clearing history must stay metadata-only.
- Recovered IDs/native auto-tabbing can collide with open windows: remap identities and reconstruct native grouping explicitly, without changing other windows.

## File ownership

| File | Responsibility |
| --- | --- |
| Create `macos/Sources/Features/Terminal/ClosedWindowSnapshot.swift` | Versioned passive window/tab envelope, capture, recovery-only identity context. |
| Create `macos/Sources/Features/Terminal/ClosedWindowStore.swift` | Bounded history, atomic persistence, configuration policy, runtime close tickets. |
| Create `macos/Sources/Features/Terminal/TerminalController+ClosedWindowRecovery.swift` | Explicit recovery and native window/tab assembly. |
| Modify `macos/Sources/Features/Terminal/TerminalController.swift` | Close capture, failure decisions, close-origin propagation, Undo consumption. |
| Modify `macos/Sources/Features/Terminal/TerminalRestorable.swift` | Share state application between AppKit restoration and recovery. |
| Modify `macos/Sources/Ghostty/Surface View/SurfaceView_AppKit.swift` | Optional recovery-only decoder identity mapping and directory fallback. |
| Modify `macos/Sources/App/AppDelegate.swift` | Own store, synchronize configuration, expose recovery actions and validate menu availability. |
| Modify `macos/Sources/App/MainMenu.xib` | Native File menu recovery/history entries. |
| Create `macos/Tests/Terminal/ClosedWindowSnapshotTests.swift`, `ClosedWindowStoreTests.swift` | Snapshot/storage/policy regression coverage. |
| Modify `macos/Tests/Terminal/TerminalControllerTests.swift`, `TerminalRestorableTests.swift` | Real native window lifecycle, identity, Undo, normal restoration regression coverage. |
| Create `macos/GhosttyUITests/GhosttyClosedWindowRecoveryTests.swift` | Red-button, zero-window menu, and restart workflow. |
| Modify `README.md` | Explain recovery, configuration, history clearing, and new-session limits. |

Existing synchronized Xcode source/test groups should pick up new files automatically; verify membership rather than editing the project unnecessarily.

## Task 1: Capture a passive whole-window snapshot

**Interfaces:** Define `ClosedWindowSnapshot: Codable, Identifiable`, with `id: UUID`, `closedAt: Date`, `frame: NSRect`, `selectedTabIndex: Int`, ordered `tabs: [Tab]`, and bounded display metadata. `Tab` contains `stateVersion: Int` and `stateData: Data`. Define `static func capture(from controller: TerminalController) throws -> ClosedWindowSnapshot`. The store consumes encoded envelopes, not live view objects.

- [ ] Add failing tests in `ClosedWindowSnapshotTests`: ordered multi-tab capture, selected tab, groups/splits/titles/colors, single-terminal capture, and rejection of non-restorable members. Assert encoding/loading the envelope retains no controllers and creates no surfaces.
- [ ] Run `macos/build.nu --action test`; confirm the new tests fail because snapshot capture is missing, rather than from unrelated environment failures.
- [ ] Implement capture using existing `TerminalRestorableState` encoding, before any surface tree is cleared. Save each state's format version explicitly. Keep display fields separate from state data so menu construction never decodes terminal states.
- [ ] Run `macos/build.nu --action test`; require passing snapshot tests and existing restoration migration tests.

## Task 2: Persist bounded history and enforce policy

**Interfaces:** `@MainActor final class ClosedWindowStore`, initialized with an explicit `directoryURL: URL` for test isolation. Expose `entries: [ClosedWindowSnapshot]`, `load() throws`, `append(_ snapshot: ClosedWindowSnapshot) throws -> CloseTicket`, `consume(id: UUID) throws`, `clear() throws`, and `setPersistenceEnabled(_ enabled: Bool) throws`. `CloseTicket` has `id: UUID` and runtime `isRestored: Bool`; a retained ticket remains valid after history pruning. The app owns one store.

- [ ] Add failing `ClosedWindowStoreTests` for A-then-B ordering, reload through a new store, 20-entry pruning, the 10 MiB encoded-file limit, oversized-single-entry rejection, and write failure preserving the previous committed file.
- [ ] Add policy/failure tests: effective always/default/never preferences, disabled persistence bypassing capture/save prompts without changing live Undo, disable/re-enable without stale records, deletion failure, clear followed by a stale callback, corrupt outer schema, unsupported tab versions, bad selected index, and excessive file size. Assert these operations create zero terminals.
- [ ] Run `macos/build.nu --action test`; confirm the new tests expose missing behavior.
- [ ] Implement the versioned file and serialized atomic mutations, owner-only permissions, validation, and runtime tickets. Keep loaded state and committed disk state consistent on mutation failure. Unknown/corrupt formats remain unavailable until explicit clearing, rather than being overwritten by a subsequent close.
- [ ] Run `macos/build.nu --action test`; require all storage/policy tests to pass using temporary directories, never the developer's real history.

## Task 3: Recover layouts through the existing terminal behavior

**Interfaces:** `@MainActor static func reopen(_ snapshot: ClosedWindowSnapshot, ghostty: Ghostty.App) throws -> [TerminalController]` on `TerminalController`. It consumes a passive snapshot and produces one visible logical window. Add a recovery-only decoder context that maps old surface UUIDs to new UUIDs; apply the same mapping to focused-surface references. Remap sidebar group UUIDs consistently across all recovered tabs. Existing launch restore uses no recovery context.

- [ ] Add failing controller/restoration tests for ordered tab/group/split restoration, titles/colors, selected tab, focus, frame/fullscreen handling, and fresh surface/group IDs while the source IDs still exist elsewhere.
- [ ] Add failure tests for missing working directories, offscreen frames, incompatible payloads, and partial construction. Assert cleanup does not register history, original windows remain untouched, and the history record is retained on failure.
- [ ] Run `macos/build.nu --action test`; confirm failures are specific to the missing recovery entry point.
- [ ] Extract shared application of decoded state from `TerminalWindowRestoration` only where needed. Decode only on explicit reopen, preserve AppKit's existing ID behavior, normalize automatic tab grouping, then join recovered tabs in saved order. Use ordinary configured startup-directory fallback and report unavailable directories after recovery.
- [ ] Run `macos/build.nu --action test`; require recovery and all existing migration/controller tests to pass.

## Task 4: Save before close and coordinate with Undo

**Interfaces:** Existing user-facing close actions remain the entry points. Add an internal explicit close-origin value for user close, normal process exit, Undo/Redo, app teardown, and recovery cleanup wherever callers currently converge. Keep capture at the responsible whole-window layer; share the returned `CloseTicket` with its Undo closure. Do not infer origin from `withConfirmation` or Undo registration state.

- [ ] Trace every caller of `closeWindowImmediately`, `closeTabImmediately`, and `closeSurface` before changing signatures. Pin which ones are deliberate whole-window/final-tab closes. Normal process exit, recovery cleanup, and undoing window creation do not generate history; a user Redo of a close does.
- [ ] Add failing controller tests for A-then-B close, final-tab close, red-button routing, one-record whole-group close, Close All Windows, canceled process confirmation, and changed membership while confirmation is pending.
- [ ] Add tests for Undo consumption, persistent recovery before stale Undo, pruning while live Undo is still valid, `undo-timeout = 0`, save failures, explicit Close Without Recovery, and last-window auto-quit. Test red-button close with persistence disabled, including after history-deletion failure: no capture/write/prompt, with existing Undo preserved. Inject the temporary store and decision handler so tests never show modal sheets or write real history.
- [ ] Run `macos/build.nu --action test`; confirm the new lifecycle assertions fail before integration.
- [ ] Record once after accepted process confirmation and before teardown. On save failure, keep the window open, default to Cancel, and offer Close Without Recovery. Re-enter the ordinary close path with fresh membership after a sheet decision. Preserve existing live Undo and make successful Undo/reopen consume their shared close ticket once.
- [ ] After terminal construction or live Undo succeeds, treat history-consumption failure separately: keep reopened terminals, mark the runtime ticket restored, hide the stale history entry, and show a warning with Retry. Explain that the old record may reappear after restart until cleanup succeeds. Test both reopen and live Undo with failed history writes and successful retry; do not roll back live terminals or claim durable exactly-once behavior during a disk failure.
- [ ] Run `macos/build.nu --action test`; require all lifecycle regressions and existing close/Undo tests to pass.

## Task 5: Expose recovery in native menus and prove the user workflow

**Interfaces:** `AppDelegate` owns `closedWindowStore` and provides `@IBAction func reopenClosedWindow(_ sender: Any?)`, `@IBAction func reopenRecentWindow(_ sender: NSMenuItem)`, and `@IBAction func clearRecentlyClosedWindows(_ sender: Any?)`. History items carry only snapshot UUIDs. Menu validation works without a key terminal window and resolves the current entry again when invoked.

- [ ] Add menu/action tests for no windows, empty history, newest-first labels, selecting older A after newer B, cleared/pruned entries referenced by stale menu items, disabled persistence, and recovery errors.
- [ ] Run `macos/build.nu --action test`; confirm the menu/action tests fail before wiring the actions.
- [ ] Add native File menu items, dynamic metadata-only history, clear action, and new-session help text. Synchronize store policy after configuration is loaded and on config reload, using the effective `NSQuitAlwaysKeepsWindows` preference. Present errors without exposing saved paths/titles in logs.
- [ ] Add `GhosttyClosedWindowRecoveryTests` using the existing isolated UI-test configuration. Cover A with multiple tabs and B with one terminal; close both through the red button, verify zero-window recovery commands, quit/relaunch, choose A, and assert its tab layout. Also cover disabled/expired Undo and canceled running-process close.
- [ ] Update `README.md` with the menu path, retained-history limits, configuration behavior, clearing, and the difference between live Undo and reopening in new sessions.
- [ ] Run `macos/build.nu --action test`, `macos/build.nu`, `swiftlint lint --strict` on changed Swift files, and `git diff --check`. Require passing unit tests/build, no relevant lint violations, and a clean whitespace check.
- [ ] In a local UI-enabled session, run `xcodebuild -project macos/Ghostty.xcodeproj -scheme Ghostty -configuration Debug -only-testing:GhosttyUITests/GhosttyClosedWindowRecoveryTests test`. Record pass/fail separately from unit results; do not claim a UI pass if the environment cannot run it.
- [ ] Manually verify horizontal tabs and sidebar layout, keyboard-only menus with zero windows, existing Cmd-Q/AppKit restoration without duplicates, and last-window auto-quit. Record any environment limits and leave implementation changes uncommitted for review.

## Planning self-review

- Coverage: the seven acceptance cases are assigned to Tasks 2 through 5; snapshot lifetime and decoding side effects are covered in Tasks 1 and 3.
- Ownership: one app store owns disk history; terminal controllers own destructive closes and recovery; existing Undo/AppKit restoration retain their current responsibilities.
- Compatibility: no Zig config/schema changes; existing terminal migration fixtures remain required. Recovery-only ID mapping is explicitly separate from launch restoration.
- Review focus: each of the five risks above has regression coverage in Tasks 1 through 5.
- Product choices were approved when implementation was requested: manual recovery, native File menu location, 20 windows/10 MiB, system preference coupling, and the explicit save-failure choice.

## Execution record

- [x] Task 1: passive whole-window snapshots using the existing codec, with version/index/identity validation and no terminal decoding during history operations.
- [x] Task 2: app-owned atomic history, 20-window/10 MiB bounds, owner-only permissions, configuration disable/clear, corruption protection, and retryable consumption failures.
- [x] Task 3: explicit layout recovery with new surface/group identities, directory fallback, screen-constrained frames, and rejection of terminal initialization failures.
- [x] Task 4: capture before deliberate whole-window/final-terminal close, shared live-Undo tickets, Close All deduplication, process confirmation/cancellation, and membership rechecks.
- [x] Task 5: native File menus, zero-window action validation, documentation, unit/native-window tests, and compiled UI test workflows.
- [ ] Full UI restart automation: XCTest timed out enabling macOS automation mode before either UI test could execute. No passing end-to-end UI restart claim is made.

### Implementation decisions

- Kept the existing checkout/build artifacts on `feat/closed-window-recovery` and preserved unrelated files. The feature was subsequently committed at the user's request.
- The red traffic-light widget uses AppKit's private `_close:` action on this macOS, which can close only the selected native tab. Its target/action now invokes the existing whole-window close handler on load and activation. Individual tab-close buttons retain their existing behavior. A real `NSButton.performClick` regression failed before this fix and passes afterward.
- Process review approves all affected terminals before closing the whole window. A later Cancel therefore preserves the intact layout.
- Explicit ScriptTerminal/AppIntent closes carry user origin independently of confirmation policy; process exits remain automatic.
- Terminal initialization is checked in the recovery-only decoding context. A failed terminal cannot be accepted as successful recovery and consume its saved entry.
- Added an optional `--only-testing` flag to `macos/build.nu` for focused native tests.
- New tests exposed reentrant target deallocation in `ExpiringUndoManager.removeAllActions`; retaining the proxies until the set mutation finishes fixes it.
- Whole-window live Undo now preserves `tabbingIdentifier` before showing restored windows. A new assertion failed deterministically before the fix; it prevents unrelated native groups from merging during restoration.
- Live Undo retains its existing PTY lifetime. The fresh reviewer recommended keeping that behavior; recovery tickets prevent duplicate resurrection.

### Verification

- Final `macos/build.nu --action test`: **340 passed, 1 failed, 1 skipped**. All 29 new recovery tests pass. Existing restoration migration and native group Undo tests pass.
- The sole failure, `TerminalTabModelTests.reservesSpaceForOnlyTheVisibleTabLayout`, was present in the pre-change baseline: it assumes a 220-pixel sidebar while this machine has a persisted width of 200.28125 pixels. The unrelated sidebar behavior was left unchanged.
- Strict SwiftLint: **zero violations across all 21 changed/new Swift files**.
- Final `macos/build.nu`: **BUILD SUCCEEDED**. Tracked and new files pass whitespace checks.
- Desktop inspection confirmed the native recovery menus and independent earlier/newer history entries. The existing user windows were not closed for restart testing. Native red-button routing and confirmation mutation/cancellation are covered by in-process AppKit tests.
- Limits: full OS-driven restart, fullscreen transitions, last-window auto-quit, and both visual layouts were not independently exercised end to end because the UI runner could not enable automation mode. The UI test source is available in `macos/GhosttyUITests/GhosttyClosedWindowRecoveryTests.swift`.
