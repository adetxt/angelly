//! The key bindings of the window actions, derived from the Ghostty keybind config so
//! that `keybind = ...` lines win over the defaults and a reload takes effect at once.
//!
//! The menu bar items and the workspace's own actions (new tab, splits, ...) need a gpui
//! binding to show a shortcut and to run before the terminal sees the key. Every binding
//! made here carries [`CONFIGURED`] so the next sync can replace exactly those and leave
//! the component library's bindings alone.

use std::rc::Rc;

use angelly_core::config::Config;
use gpui_kit::*;

use crate::platform::menus::gpui_keystroke;
use crate::splits::{
    ClosePane, EqualizeSplits, GotoSplitDown, GotoSplitLeft, GotoSplitNext, GotoSplitPrevious, GotoSplitRight, GotoSplitUp,
    ResizeSplitDown, ResizeSplitLeft, ResizeSplitRight, ResizeSplitUp, SplitDown, SplitLeft, SplitRight, SplitUp,
    ToggleSplitZoom,
};
use crate::terminal_view::{Copy, Paste, SelectAll};
use crate::workspace::{CloseTab, NewTab, NextTab, PreviousTab};

/// Marks the bindings that come from the config.
pub const CONFIGURED: KeyBindingMetaIndex = KeyBindingMetaIndex(0xC0F1);

pub fn terminal_input_bindings() -> [KeyBinding; 2] {
    // Let the terminal encode Tab before the toolkit moves focus.
    [
        KeyBinding::new("tab", NoAction {}, Some("Terminal")),
        KeyBinding::new("shift-tab", NoAction {}, Some("Terminal")),
    ]
}

struct Entry {
    /// The Ghostty action string whose trigger the binding takes.
    action: &'static str,
    context: &'static str,
    make: fn() -> Box<dyn Action>,
}

macro_rules! entries {
    ($($action:literal, $context:literal => $gpui:expr;)*) => {
        &[$(Entry { action: $action, context: $context, make: || Box::new($gpui) }),*]
    };
}

const ENTRIES: &[Entry] = entries! {
    "copy_to_clipboard", "Terminal" => Copy;
    "paste_from_clipboard", "Terminal" => Paste;
    "select_all", "Terminal" => SelectAll;
    "new_tab", "Workspace" => NewTab;
    "close_tab:this", "Workspace" => CloseTab;
    "next_tab", "Workspace" => NextTab;
    "previous_tab", "Workspace" => PreviousTab;
    "close_surface", "Workspace" => ClosePane;
    "new_split:right", "Workspace" => SplitRight;
    "new_split:down", "Workspace" => SplitDown;
    "new_split:left", "Workspace" => SplitLeft;
    "new_split:up", "Workspace" => SplitUp;
    "goto_split:previous", "Workspace" => GotoSplitPrevious;
    "goto_split:next", "Workspace" => GotoSplitNext;
    "goto_split:up", "Workspace" => GotoSplitUp;
    "goto_split:down", "Workspace" => GotoSplitDown;
    "goto_split:left", "Workspace" => GotoSplitLeft;
    "goto_split:right", "Workspace" => GotoSplitRight;
    "resize_split:up,10", "Workspace" => ResizeSplitUp;
    "resize_split:down,10", "Workspace" => ResizeSplitDown;
    "resize_split:left,10", "Workspace" => ResizeSplitLeft;
    "resize_split:right,10", "Workspace" => ResizeSplitRight;
    "equalize_splits", "Workspace" => EqualizeSplits;
    "toggle_split_zoom", "Workspace" => ToggleSplitZoom;
};

/// The `(action, context, gpui keystroke)` triples the config asks for. Actions without a
/// plain, non-global trigger get none, so their keys fall through to the terminal.
pub fn resolved(config: &Config) -> Vec<(&'static str, &'static str, String)> {
    ENTRIES
        .iter()
        .filter_map(|entry| {
            let binding = config
                .keybinds()
                .trigger_for_action(entry.action)
                .filter(|binding| !binding.flags.global && !binding.is_sequence())?;
            Some((entry.action, entry.context, gpui_keystroke(binding.trigger())?))
        })
        .collect()
}

/// Replaces every config-derived binding with the ones the current config asks for, and
/// refreshes the menu bar's shortcuts.
pub fn sync(cx: &mut App) {
    remove_configured(cx);
    crate::platform::menus::sync_shortcuts(cx);
    let Some(config) = cx.try_global::<crate::theme::Theme>().map(|theme| theme.config.clone()) else { return };
    let mut bindings = Vec::new();
    for (action, context, keystroke) in resolved(&config) {
        let Some(entry) = ENTRIES.iter().find(|entry| entry.action == action) else { continue };
        let predicate = KeyBindingContextPredicate::parse(context).ok().map(Rc::new);
        if let Ok(binding) = KeyBinding::load(&keystroke, (entry.make)(), predicate, false, None, cx) {
            bindings.push(binding.with_meta(CONFIGURED));
        }
    }
    cx.bind_keys(bindings);
}

/// Drops the bindings an earlier sync made.
fn remove_configured(cx: &mut App) {
    let keymap = cx.key_bindings();
    let mut keymap = keymap.borrow_mut();
    let kept: Vec<KeyBinding> = keymap.bindings().filter(|binding| binding.meta() != Some(CONFIGURED)).cloned().collect();
    keymap.clear();
    keymap.add_bindings(kept);
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use angelly_core::config::{ColorScheme, Config, Env};

    use super::*;

    #[test]
    fn tab_reaches_terminal_without_disabling_navigation_in_panels() {
        let mut keymap = Keymap::default();
        keymap.add_bindings([
            KeyBinding::new("tab", NextTab, Some("Root")),
            KeyBinding::new("shift-tab", PreviousTab, Some("Root")),
        ]);
        keymap.add_bindings(terminal_input_bindings());
        for key in ["tab", "shift-tab"] {
            let input = [Keystroke::parse(key).unwrap()];
            let root = KeyContext::parse("Root").unwrap();
            let terminal = KeyContext::parse("Terminal").unwrap();
            assert!(keymap.bindings_for_input(&input, &[root.clone(), terminal]).0.is_empty());
            let panel = KeyContext::parse("Input").unwrap();
            assert_eq!(keymap.bindings_for_input(&input, &[root, panel]).0.len(), 1);
        }
    }

    fn config(text: &str) -> Config {
        Config::from_str(text, &Env::from_process(), ColorScheme::Dark)
    }

    fn keystroke(config: &Config, action: &str) -> Option<String> {
        resolved(config).into_iter().find(|(name, ..)| *name == action).map(|(_, _, keys)| keys)
    }

    #[test]
    fn defaults_bind_the_split_actions() {
        let config = config("");
        let split = keystroke(&config, "new_split:right").expect("a default split binding");
        assert!(split.ends_with('d'));
        assert!(keystroke(&config, "toggle_split_zoom").is_some());
    }

    #[test]
    fn a_user_binding_replaces_the_default() {
        let config = config("keybind = ctrl+alt+x=new_split:right\n");
        assert_eq!(keystroke(&config, "new_split:right").as_deref(), Some("ctrl-alt-x"));
    }

    #[test]
    fn an_unbound_action_gets_no_binding() {
        let defaults = config("");
        let default = keystroke(&defaults, "new_split:right").unwrap();
        let trigger = default.replace("cmd", "super").replace('-', "+");
        let config = config(&format!("keybind = {trigger}=unbind\n"));
        assert_eq!(keystroke(&config, "new_split:right"), None);
    }

    #[test]
    fn sequences_and_global_bindings_stay_with_the_terminal() {
        let config = config("keybind = ctrl+a>n=new_split:right\n");
        assert_ne!(keystroke(&config, "new_split:right").as_deref(), Some("ctrl-a"));
        let config = self::config("keybind = global:ctrl+alt+x=new_tab\n");
        assert_ne!(keystroke(&config, "new_tab").as_deref(), Some("ctrl-alt-x"));
    }
}
