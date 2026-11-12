//! The application menu bar (spec section 18), the dock menu and the default key
//! bindings of the actions it introduces.
//!
//! Actions that already exist in the workspace and terminal views are used as they
//! are. Everything else is declared here and forwarded to the [`Host`] as a
//! [`Command`] by a global handler, so a view can still claim an action by handling
//! it on its own element.

use std::rc::Rc;

use gpui_kit::*;

use angelly_core::config::{Trigger, TriggerKey};

use super::command::{self, Command};
use crate::theme::Theme;
use crate::splits::{
    ClosePane, EqualizeSplits, GotoSplitDown, GotoSplitLeft, GotoSplitNext, GotoSplitPrevious, GotoSplitRight, GotoSplitUp,
    ResizeSplitDown, ResizeSplitLeft, ResizeSplitRight, ResizeSplitUp, SplitDown, SplitLeft, SplitRight, SplitUp,
    ToggleSplitZoom,
};
use crate::panels::{CheckForUpdates, OpenSettings as Settings, ShowAbout as About, ToggleCommandPalette as CommandPalette};
use crate::tabs::ReopenClosedWindow;
use crate::terminal_view::{Copy, Paste, SelectAll};
use crate::workspace::{CloseTab, NewTab, NextTab, PreviousTab, ToggleSidebar};

macro_rules! forwarded_actions {
    ($($name:ident => $command:expr),* $(,)?) => {
        actions!(angelly, [$($name),*]);

        fn register_forwarded(cx: &mut App) {
            $(cx.on_action(|_: &$name, cx| command::perform($command, cx));)*
        }
    };
}

forwarded_actions! {
    Quit => Command::Quit,
    ToggleQuickTerminal => Command::ToggleQuickTerminal,
    ReloadConfig => Command::ReloadConfig,
    NewWindow => Command::NewWindow,
    SaveWorkspace => Command::SaveWorkspace,
    OpenWorkspace => Command::OpenWorkspace,
    ClearRecentlyClosed => Command::ClearRecentlyClosed,
    CloseWindow => Command::CloseWindow,
    CloseAllWindows => Command::CloseAllWindows,
    Undo => Command::Undo,
    Redo => Command::Redo,
    PasteSelection => Command::PasteSelection,
    Find => Command::Find,
    FindNext => Command::FindNext,
    FindPrevious => Command::FindPrevious,
    HideFindBar => Command::HideFindBar,
    UseSelectionForFind => Command::UseSelectionForFind,
    JumpToSelection => Command::JumpToSelection,
    ResetFontSize => Command::ResetFontSize,
    IncreaseFontSize => Command::IncreaseFontSize,
    DecreaseFontSize => Command::DecreaseFontSize,
    ChangeTabTitle => Command::ChangeTabTitle,
    ChangeTerminalTitle => Command::ChangeTerminalTitle,
    ToggleReadonly => Command::ToggleReadonly,
    ToggleInspector => Command::ToggleInspector,
    ReturnToDefaultSize => Command::ReturnToDefaultSize,
    ToggleFloatOnTop => Command::ToggleFloatOnTop,
    UseAsDefaultSize => Command::UseAsDefaultSize,
}

actions!(
    angelly,
    [
        ToggleSecureInput,
        MakeDefaultTerminal,
        ToggleVisibility,
        Hide,
        HideOthers,
        ShowAll,
        Minimize,
        Zoom,
        ToggleFullscreen,
        BringAllToFront,
        OpenHelp,
    ]
);

#[derive(Clone, PartialEq, Action)]
#[action(namespace = angelly, no_json)]
pub struct OpenRecentlyClosed {
    pub index: usize,
}

const HELP_URL: &str = "https://ghostty.org/docs";

/// Titles of the windows the history can reopen, newest first. Set by the app when the
/// history changes; an empty list shows the "No Recently Closed Windows" placeholder.
#[derive(Default)]
pub struct RecentlyClosed {
    pub entries: Vec<String>,
    pub enabled: bool,
}

impl Global for RecentlyClosed {}

pub fn set_recently_closed(entries: Vec<String>, enabled: bool, cx: &mut App) {
    cx.set_global(RecentlyClosed { entries, enabled });
    install(cx);
}

pub fn init(cx: &mut App) {
    if !cx.has_global::<RecentlyClosed>() {
        cx.set_global(RecentlyClosed::default());
    }
    register_forwarded(cx);
    register_platform(cx);
    cx.on_action(|action: &OpenRecentlyClosed, cx| command::perform(Command::OpenRecentlyClosed(action.index), cx));
    sync_shortcuts(cx);
    install(cx);
}

fn register_platform(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &BringAllToFront, cx| cx.activate(true));
    cx.on_action(|_: &OpenHelp, cx| cx.open_url(HELP_URL));
    cx.on_action(|_: &Minimize, cx| with_active_window(cx, |window| window.minimize_window()));
    cx.on_action(|_: &Zoom, cx| with_active_window(cx, |window| window.zoom_window()));
    cx.on_action(|_: &ToggleFullscreen, cx| with_active_window(cx, |window| window.toggle_fullscreen()));
    cx.on_action(|_: &ToggleSecureInput, cx| {
        super::secure_input::toggle(cx);
        install(cx);
    });
    cx.on_action(|_: &MakeDefaultTerminal, _| super::default_terminal::make_default());
    cx.on_action(|_: &ToggleVisibility, cx| super::visibility::toggle(cx));
    cx.on_action(|_: &NewTab, cx| command::perform(Command::NewTab, cx));
    cx.on_action(|_: &CloseTab, cx| command::perform(Command::CloseTab, cx));
    cx.on_action(|_: &ToggleSidebar, cx| command::perform(Command::ToggleSidebar, cx));
}

pub(super) fn with_active_window(cx: &mut App, f: impl FnOnce(&mut Window)) {
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, |_, window, _| f(window));
    }
}

/// Rebuilds the menu bar and the dock menu from the current state. Call it after
/// anything the menus display changes (secure input, float on top, closed windows).
pub fn install(cx: &mut App) {
    let menus = build(cx);
    cx.set_menus(menus);
    cx.set_dock_menu(vec![
        MenuItem::action("New Window", NewWindow),
        MenuItem::action("New Tab", NewTab),
    ]);
}

fn build(cx: &App) -> Vec<Menu> {
    let secure = super::secure_input::is_enabled();
    let floating = super::float_on_top::is_floating();
    let recent = cx.try_global::<RecentlyClosed>();

    let recent_items = match recent {
        Some(r) if !r.enabled => vec![MenuItem::action("Window Recovery Is Disabled", ReopenClosedWindow).disabled(true)],
        Some(r) if r.entries.is_empty() => {
            vec![MenuItem::action("No Recently Closed Windows", ReopenClosedWindow).disabled(true)]
        }
        Some(r) => r
            .entries
            .iter()
            .enumerate()
            .map(|(index, title)| MenuItem::action(title.clone(), OpenRecentlyClosed { index }))
            .chain([MenuItem::separator(), MenuItem::action("Clear Recently Closed Windows", ClearRecentlyClosed)])
            .collect(),
        None => Vec::new(),
    };

    vec![
        Menu::new("Angelly").items([
            MenuItem::action("About Angelly", About),
            MenuItem::action("Check for Updates\u{2026}", CheckForUpdates),
            MenuItem::separator(),
            MenuItem::action("Preferences\u{2026}", Settings),
            MenuItem::action("Reload Configuration", ReloadConfig),
            MenuItem::separator(),
            MenuItem::action("Secure Keyboard Entry", ToggleSecureInput).checked(secure),
            MenuItem::action("Make Angelly the Default Terminal", MakeDefaultTerminal),
            MenuItem::separator(),
            MenuItem::os_submenu("Services", SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action("Hide Angelly", Hide),
            MenuItem::action("Hide Others", HideOthers),
            MenuItem::action("Show All", ShowAll),
            MenuItem::separator(),
            MenuItem::action("Quit Angelly", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Window", NewWindow),
            MenuItem::action("New Tab", NewTab),
            MenuItem::separator(),
            MenuItem::action("Save Workspace\u{2026}", SaveWorkspace),
            MenuItem::action("Open Workspace\u{2026}", OpenWorkspace),
            MenuItem::action("Reopen Closed Window", ReopenClosedWindow),
            MenuItem::submenu(Menu::new("Recently Closed Windows").items(recent_items)),
            MenuItem::separator(),
            MenuItem::action("Split Right", SplitRight),
            MenuItem::action("Split Left", SplitLeft),
            MenuItem::action("Split Down", SplitDown),
            MenuItem::action("Split Up", SplitUp),
            MenuItem::separator(),
            MenuItem::action("Close", ClosePane),
            MenuItem::action("Close Tab", CloseTab),
            MenuItem::action("Close Window", CloseWindow),
            MenuItem::action("Close All Windows", CloseAllWindows),
        ]),
        Menu::new("Edit").items([
            MenuItem::os_action("Undo", Undo, OsAction::Undo),
            MenuItem::os_action("Redo", Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action("Copy", Copy, OsAction::Copy),
            MenuItem::os_action("Paste", Paste, OsAction::Paste),
            MenuItem::action("Paste Selection", PasteSelection),
            MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Find").items([
                MenuItem::action("Find\u{2026}", Find),
                MenuItem::action("Find Next", FindNext),
                MenuItem::action("Find Previous", FindPrevious),
                MenuItem::action("Hide Find Bar", HideFindBar),
                MenuItem::separator(),
                MenuItem::action("Use Selection for Find", UseSelectionForFind),
                MenuItem::action("Jump to Selection", JumpToSelection),
            ])),
        ]),
        Menu::new("View").items([
            MenuItem::action("Reset Font Size", ResetFontSize),
            MenuItem::action("Increase Font Size", IncreaseFontSize),
            MenuItem::action("Decrease Font Size", DecreaseFontSize),
            MenuItem::separator(),
            MenuItem::action("Command Palette", CommandPalette),
            MenuItem::separator(),
            MenuItem::action("Change Tab Title\u{2026}", ChangeTabTitle),
            MenuItem::action("Toggle Tab Sidebar", ToggleSidebar),
            MenuItem::action("Change Terminal Title\u{2026}", ChangeTerminalTitle),
            MenuItem::action("Terminal Read-only", ToggleReadonly),
            MenuItem::separator(),
            MenuItem::action("Quick Terminal", ToggleQuickTerminal),
            MenuItem::action("Terminal Inspector", ToggleInspector),
        ]),
        Menu::new("Window").items([
            MenuItem::action("Minimize", Minimize),
            MenuItem::action("Zoom", Zoom),
            MenuItem::action("Toggle Full Screen", ToggleFullscreen),
            MenuItem::separator(),
            MenuItem::action("Show/Hide All Terminals", ToggleVisibility),
            MenuItem::separator(),
            MenuItem::action("Zoom Split", ToggleSplitZoom),
            MenuItem::action("Select Previous Split", GotoSplitPrevious),
            MenuItem::action("Select Next Split", GotoSplitNext),
            MenuItem::submenu(Menu::new("Select Split").items([
                MenuItem::action("Above", GotoSplitUp),
                MenuItem::action("Below", GotoSplitDown),
                MenuItem::action("Left", GotoSplitLeft),
                MenuItem::action("Right", GotoSplitRight),
            ])),
            MenuItem::submenu(Menu::new("Resize Split").items([
                MenuItem::action("Equalize Splits", EqualizeSplits),
                MenuItem::separator(),
                MenuItem::action("Move Divider Up", ResizeSplitUp),
                MenuItem::action("Move Divider Down", ResizeSplitDown),
                MenuItem::action("Move Divider Left", ResizeSplitLeft),
                MenuItem::action("Move Divider Right", ResizeSplitRight),
            ])),
            MenuItem::separator(),
            MenuItem::action("Select Next Tab", NextTab),
            MenuItem::action("Select Previous Tab", PreviousTab),
            MenuItem::separator(),
            MenuItem::action("Return To Default Size", ReturnToDefaultSize),
            MenuItem::action("Float on Top", ToggleFloatOnTop).checked(floating),
            MenuItem::action("Use as Default", UseAsDefaultSize),
            MenuItem::separator(),
            MenuItem::action("Bring All to Front", BringAllToFront),
        ]),
        Menu::new("Help").items([MenuItem::action("Angelly Help", OpenHelp)]),
    ]
}

/// Key bindings that exist only to give the menu items their shortcuts and do not come
/// from the Ghostty keybind config.
#[cfg(target_os = "macos")]
fn fixed_bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("ctrl-cmd-f", ToggleFullscreen, None),
        KeyBinding::new("cmd-?", OpenHelp, None),
    ]
}

#[cfg(not(target_os = "macos"))]
fn fixed_bindings() -> Vec<KeyBinding> {
    vec![KeyBinding::new("ctrl-enter", ToggleFullscreen, None)]
}

/// The gpui spelling of a keybind trigger, e.g. `super+shift+arrow_left` as
/// `cmd-shift-left`. `None` for triggers gpui has no key for.
pub fn gpui_keystroke(trigger: &Trigger) -> Option<String> {
    let key = match &trigger.key {
        TriggerKey::Unicode(c) => c.to_string(),
        TriggerKey::Physical(name) => physical_key(name)?,
        TriggerKey::CatchAll => return None,
    };
    let mut parts = Vec::new();
    if trigger.mods.ctrl {
        parts.push("ctrl");
    }
    if trigger.mods.alt {
        parts.push("alt");
    }
    if trigger.mods.shift {
        parts.push("shift");
    }
    if trigger.mods.super_key {
        parts.push(if cfg!(target_os = "macos") { "cmd" } else { "super" });
    }
    parts.push(&key);
    Some(parts.join("-"))
}

fn physical_key(name: &str) -> Option<String> {
    let key = match name {
        "backquote" => "`",
        "backslash" => "\\",
        "bracket_left" => "[",
        "bracket_right" => "]",
        "comma" => ",",
        "equal" => "=",
        "minus" => "-",
        "period" => ".",
        "quote" => "'",
        "semicolon" => ";",
        "slash" => "/",
        "arrow_left" => "left",
        "arrow_right" => "right",
        "arrow_up" => "up",
        "arrow_down" => "down",
        "page_up" => "pageup",
        "page_down" => "pagedown",
        "enter" | "space" | "tab" | "escape" | "backspace" | "delete" | "home" | "end" | "insert" => name,
        _ => {
            if let Some(letter) = name.strip_prefix("key_").or_else(|| name.strip_prefix("digit_")) {
                return Some(letter.to_owned());
            }
            if name.starts_with('f') && name[1..].parse::<u8>().is_ok() {
                return Some(name.to_owned());
            }
            return None;
        }
    };
    Some(key.to_owned())
}

/// Where a trigger applies. A key pressed without ctrl, alt or super types text, so
/// it belongs to the terminal: bound everywhere it would take Escape from every text
/// field and overlay.
fn binding_context(trigger: &Trigger) -> Option<Rc<KeyBindingContextPredicate>> {
    let mods = &trigger.mods;
    let typing = !(mods.ctrl || mods.alt || mods.super_key);
    typing.then(|| KeyBindingContextPredicate::parse("Terminal").ok().map(Rc::new)).flatten()
}

macro_rules! synced {
    ($cx:expr, $config:expr, $($ghostty:literal => $action:expr),* $(,)?) => {{
        let mut bindings = Vec::new();
        $(
            if let Some((keys, context)) = $config
                .keybinds()
                .trigger_for_action($ghostty)
                .filter(|binding| !binding.flags.global && !binding.is_sequence())
                .and_then(|binding| Some((gpui_keystroke(binding.trigger())?, binding_context(binding.trigger()))))
            {
                bindings.extend(
                    KeyBinding::load(&keys, Box::new($action), context, false, None, &*$cx)
                        .ok()
                        .map(|binding| binding.with_meta(crate::shortcuts::CONFIGURED)),
                );
            }
        )*
        $cx.bind_keys(bindings);
    }};
}

/// Binds the keybinds the user configured for the menu's actions, so the menu shows and
/// honors them (the equivalent of Ghostty's `MenuShortcutManager`). Bindings added by
/// an earlier call stay until the app restarts.
pub fn sync_shortcuts(cx: &mut App) {
    cx.bind_keys(fixed_bindings().into_iter().map(|binding| binding.with_meta(crate::shortcuts::CONFIGURED)));
    let Some(theme) = cx.try_global::<Theme>() else { return };
    let config = theme.config.clone();
    synced!(
        cx, config,
        "new_window" => NewWindow,
        "close_window" => CloseWindow,
        "close_all_windows" => CloseAllWindows,
        "undo" => Undo,
        "redo" => Redo,
        "paste_from_selection" => PasteSelection,
        "start_search" => Find,
        "navigate_search:next" => FindNext,
        "navigate_search:previous" => FindPrevious,
        "end_search" => HideFindBar,
        "search_selection" => UseSelectionForFind,
        "scroll_to_selection" => JumpToSelection,
        "reset_font_size" => ResetFontSize,
        "increase_font_size:1" => IncreaseFontSize,
        "decrease_font_size:1" => DecreaseFontSize,
        "toggle_command_palette" => CommandPalette,
        "prompt_tab_title" => ChangeTabTitle,
        "prompt_surface_title" => ChangeTerminalTitle,
        "toggle_readonly" => ToggleReadonly,
        "toggle_quick_terminal" => ToggleQuickTerminal,
        "inspector:toggle" => ToggleInspector,
        "toggle_visibility" => ToggleVisibility,
        "reload_config" => ReloadConfig,
        "open_config" => Settings,
        "check_for_updates" => CheckForUpdates,
        "quit" => Quit,
    );
}

#[cfg(test)]
mod tests {
    use angelly_core::config::{Mods, Trigger, TriggerKey};

    use super::{binding_context, gpui_keystroke};

    #[test]
    fn only_bare_keys_are_limited_to_the_terminal() {
        let context = |text: &str| binding_context(&Trigger::parse(text).unwrap());
        assert!(context("escape").is_some());
        assert!(context("shift+f1").is_some());
        assert!(context("super+f").is_none());
        assert!(context("ctrl+n").is_none());
    }

    #[test]
    fn triggers_become_gpui_keystrokes() {
        let trigger = Trigger::parse("super+shift+arrow_left").unwrap();
        let expected = if cfg!(target_os = "macos") { "shift-cmd-left" } else { "shift-super-left" };
        assert_eq!(gpui_keystroke(&trigger).unwrap(), expected);
        let trigger = Trigger { key: TriggerKey::Unicode(','), mods: Mods::NONE.ctrl() };
        assert_eq!(gpui_keystroke(&trigger).unwrap(), "ctrl-,");
        let trigger = Trigger::parse("alt+f4").unwrap();
        assert_eq!(gpui_keystroke(&trigger).unwrap(), "alt-f4");
        let trigger = Trigger::parse("ctrl+page_down").unwrap();
        assert_eq!(gpui_keystroke(&trigger).unwrap(), "ctrl-pagedown");
    }
}
