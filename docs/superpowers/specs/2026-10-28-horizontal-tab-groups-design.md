# Horizontal tab groups

The approved approach is a custom horizontal tab strip with Chrome-style grouping, backed by native macOS tab windows. Keep the vertical sidebar available through its existing toggle; show one tab interface at a time.

Reuse the existing group metadata and restoration format. Both layouts share selection, ordering, group membership, rename, colors, collapse, drag operations, and tab menus. New tabs remain last and ungrouped. Groups can be closed through the existing process-confirmation and undo mechanisms.

The horizontal strip occupies a compact row above the terminal content. Group labels remain visible when collapsed; expanded groups have a color line connecting their tabs. Tabs have title, selection, close, and bell indicators. Overflow scrolls within the strip and follows selection. Use native semantic colors, system typography, keyboard-accessible buttons, and existing title-edit shortcuts. Native window handling continues to own fullscreen, tab selection/order, and shortcuts.

## Approaches

- Chosen: replace the visible horizontal bar, sharing the existing group model and native tab lifecycle. This adds the requested interactions while retaining restoration and window behavior.
- Rejected: replace native tab windows entirely. That would require unrelated rewrites of fullscreen, AppleScript, shortcuts, window transfers, and restoration.

## Bounds

No nested groups, pinned tabs, synchronization, new dependencies, or new configuration. Dragging is supported within the current window's tab set, matching the existing sidebar. Preserve the existing native move-to-window actions. Closing groups must honor running-process confirmation, cancellation, and undo.

Visual direction: existing macOS app chrome with Chrome's group behavior. ENERGY 1 / RHYTHM 1 / MOTION 1; selection and group identity are the focal cues, with no decorative assets.
