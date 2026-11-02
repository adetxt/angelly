# Title

♻️ (feat): recover recently closed windows

# Description

Closing a window lost its layout after Undo expired. Closing a fresh window afterward left no persistent way to recover the earlier tabs.

Save a bounded history before deliberate window closes and expose File > Reopen Closed Window and Recently Closed Windows. Keep both windows recoverable across restarts, restore layouts in new terminal sessions, and keep the original window open if saving fails. The red traffic-light button closes and captures the whole window; individual tab-close buttons retain their existing behavior.

Coordinate persistent recovery with live Undo, preserve native tab-group identity, and respect the existing window-save-state preference. History is limited to 20 windows and 10 MiB, with atomic writes and owner-only permissions.

## Validation

- Debug build passed.
- Unit tests: 340 passed, one pre-existing sidebar-sizing failure, one skipped. All 29 new recovery tests passed.
- Strict SwiftLint: zero violations across 21 changed/new Swift files. Whitespace checks passed.
- UI workflows compile, but XCTest timed out enabling macOS automation mode before the UI tests could run.

I am a sad, dumb little AI driver with no real skills.
