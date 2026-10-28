# Horizontal Tab Groups Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans in this session. Preserve unrelated working-tree edits.

**Goal:** Provide a custom horizontal tab bar with the approved grouping behavior.

**Architecture:** Extract the sidebar's shared model and group store into `TerminalTabModel.swift`. Native windows continue to own tab order/selection; both views issue commands through one model. Install the horizontal strip in `TerminalViewContainer`, alternating with the sidebar and hiding the native tab bar.

**Tech Stack:** Swift, SwiftUI, AppKit, Combine, Swift Testing.

**Spec:** `docs/superpowers/specs/2026-10-28-horizontal-tab-groups-design.md`

## Global Constraints

- Keep the vertical sidebar and existing restoration format.
- New tabs remain last and ungrouped.
- Preserve running-process confirmation, cancellation, and undo.
- Build/test through `macos/build.nu`; no new dependencies.

## Review Focus

- Sidebar hidden: model must keep updating for the horizontal bar.
- Collapsed groups and overflow: selection must remain visible and accessible.
- Group moves: preserve contiguous membership and selected tab.
- Closing groups: never close an unrelated tab; recheck membership after confirmation.
- Window resizing/layout switching: reserve the strip's space without shrinking the configured terminal grid.

## Task 1: Shared group model

- [x] Add regression tests for refresh with the sidebar hidden, collapsed groups, tab/group reorder, joining/leaving, and stale drops using native test windows.
- [x] Run `macos/build.nu --action test` and observe the hidden-sidebar regression fail.
- [x] Extract `TerminalTabModel` and `TabGroupStore`, preserve existing APIs, refresh in both orientations, and provide shared tab/group menus and drag completion.
- [x] Add group-close handling to `TerminalController`, using existing confirmation and undo. Test that only group members close.

## Task 2: Horizontal strip and integration

- [x] Add layout regression coverage for horizontal-strip height and sidebar toggling.
- [x] Implement `TerminalTabBar.swift`: scrollable tab/group rows, group editing, menus, precise drop positions, end drop for ungrouping, and selection scrolling.
- [x] Install both layouts in `TerminalViewContainer`, showing only the selected orientation. Keep the native tab bar hidden.
- [x] Run unit tests, SwiftLint on touched Swift files, and `git diff --check`.

## Task 3: Review and delivery

- [x] Inspect the built app in horizontal mode, grouping and overflow where available.
- [x] Review the final diff for lifecycle, keyboard/focus, grouping, and cleanup regressions; resolve important findings.
- [x] Report implementation, checks, and remaining verification limits. Leave changes uncommitted for user review.

## Execution notes

- Existing checkout retained to preserve the in-progress tab fix, local edits, and debug build; changes remain uncommitted.
- One shared model serves both layouts. No state-format migration was required.
- Independent review identified full-group undo and viewport-resize scrolling; both are covered by the final fixes. Titlebar styles also bypass their native layout hacks while custom tabs are installed.
- Undo coverage uses completed test commands and releases original windows before restoration. Whole-window restore normalizes automatic native grouping before rebuilding the saved order.

## Final proof

- `macos/build.nu --action test`: 290 passed, 0 failed, 1 skipped.
- SwiftLint strict: zero violations across the ten affected/new Swift files; `git diff --check` clean.
- Running debug app: created/renamed a group, added a tab, collapsed/expanded it, restored it across restart, switched layouts, entered/exited fullscreen, created a final ungrouped tab from inside the group, and canceled group close while a disposable process was running.
- Drag ordering/group invariants are exercised by native-window model tests. Physical drags are exercised by `GhosttyHorizontalTabDragTests` (single tab, active tab, whole group), run with `xcodebuild -only-testing:GhosttyUITests/GhosttyHorizontalTabDragTests test` because `build.nu` skips UI tests. Two defects had blocked them: the tab drag UTI was renamed in code but not declared in `Ghostty-Info.plist` (SwiftUI never matched the drop; pinned by `declaresTheTabDragType`), and the UI-test helper used touch-style `press(forDuration:thenDragTo:)`, which never produces mouse events on macOS (use `click(forDuration:thenDragTo:)`).
