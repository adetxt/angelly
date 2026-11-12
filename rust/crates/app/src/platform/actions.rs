//! Ghostty action strings (`new_split:right`, `toggle_quick_terminal`, ...) mapped to
//! the application's GPUI actions. Used by global keybinds, the AppleScript
//! `perform action` command and the Shortcuts keybind intent.

use angelly_core::config::Action as KeybindAction;
use gpui_kit::*;

use super::menus::*;
use crate::keybinds::{Command, Perform};
use crate::terminal_view::{Copy, Paste, SelectAll};

fn boxed(action: impl Action) -> Option<Box<dyn Action>> {
    Some(Box::new(action))
}

/// The GPUI action behind a Ghostty action string, or `None` when the app has no
/// equivalent for it.
pub fn lookup(text: &str) -> Option<Box<dyn Action>> {
    let action = KeybindAction::parse(text).ok()?;
    if let Some(Command::App(app)) = Command::parse(&action) {
        return super::window_actions::split_action(&app).or_else(|| boxed(Perform(app)));
    }
    let param = action.param.as_deref();
    let direction = param.and_then(|p| p.split(',').next());
    match (action.name.as_str(), direction) {
        ("increase_font_size", _) => boxed(IncreaseFontSize),
        ("decrease_font_size", _) => boxed(DecreaseFontSize),
        ("reset_font_size", _) => boxed(ResetFontSize),
        ("copy_to_clipboard", _) => boxed(Copy),
        ("paste_from_clipboard", _) => boxed(Paste),
        ("paste_from_selection", _) => boxed(PasteSelection),
        ("select_all", _) => boxed(SelectAll),
        ("undo", _) => boxed(Undo),
        ("redo", _) => boxed(Redo),
        ("start_search", _) => boxed(Find),
        ("search_selection", _) => boxed(UseSelectionForFind),
        ("navigate_search", Some("next")) => boxed(FindNext),
        ("navigate_search", Some("previous")) => boxed(FindPrevious),
        ("end_search", _) => boxed(HideFindBar),
        ("scroll_to_selection", _) => boxed(JumpToSelection),
        ("toggle_readonly", _) => boxed(ToggleReadonly),
        ("inspector", Some("toggle")) => boxed(ToggleInspector),
        _ => None,
    }
}

/// Runs an action for a global keybind: the app is in the background, so the action
/// goes to the active window when there is one and to the global handlers otherwise.
pub fn perform_global(text: &str, cx: &mut App) {
    let Some(action) = lookup(text) else { return };
    cx.dispatch_action(&*action);
}

/// Dispatches `text` in the window that holds the target terminal. Returns whether the
/// action exists and something is listening for it.
pub fn perform_in_window(window: AnyWindowHandle, text: &str, cx: &mut App) -> bool {
    let Some(action) = lookup(text) else { return false };
    window
        .update(cx, |_, window, cx| {
            let available = window.is_action_available(&*action, cx);
            if available {
                window.dispatch_action(action, cx);
            }
            available
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::lookup;

    #[test]
    fn known_action_strings_resolve() {
        assert!(lookup("new_window").is_some());
        assert!(lookup("new_split:right").is_some());
        assert!(lookup("goto_split:previous").is_some());
        assert!(lookup("resize_split:up,10").is_some());
        assert!(lookup("toggle_quick_terminal").is_some());
    }

    #[test]
    fn unknown_or_unsupported_strings_do_not() {
        assert!(lookup("not_an_action").is_none());
        assert!(lookup("new_split:sideways").is_none());
        assert!(lookup("").is_none());
    }
}
