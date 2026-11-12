//! Application-level commands shared by the menu bar, the dock menu, services and
//! scripting. The platform layer turns every entry point into a [`Command`] and
//! hands it to the [`Host`], which knows about workspaces and terminals.

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::{App, Global};

use super::automation::{
    CommandInfo, Created, Detail, Id, Input, Place, Snapshot, SurfaceConfig, TitleTarget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Right,
    Left,
    Down,
    Up,
}

impl Direction {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "right" => Some(Self::Right),
            "left" => Some(Self::Left),
            "down" => Some(Self::Down),
            "up" => Some(Self::Up),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Quit,
    ToggleQuickTerminal,
    ReloadConfig,
    NewWindow,
    NewTab,
    OpenDirectories { paths: Vec<PathBuf>, tabs: bool },
    /// Run an executable or script in a new terminal; the user already confirmed.
    RunFile { path: PathBuf, tab: bool },
    SaveWorkspace,
    OpenWorkspace,
    OpenRecentlyClosed(usize),
    ClearRecentlyClosed,
    CloseTab,
    CloseWindow,
    CloseAllWindows,
    Undo,
    Redo,
    PasteSelection,
    Find,
    FindNext,
    FindPrevious,
    HideFindBar,
    UseSelectionForFind,
    JumpToSelection,
    ResetFontSize,
    IncreaseFontSize,
    DecreaseFontSize,
    ChangeTabTitle,
    ChangeTerminalTitle,
    ToggleSidebar,
    ToggleReadonly,
    ToggleInspector,
    ReturnToDefaultSize,
    ToggleFloatOnTop,
    UseAsDefaultSize,
}

const UNSUPPORTED: &str = "Not supported by this build.";

/// What the platform layer needs from the application. The app crate provides one
/// implementation that maps commands onto its workspaces. The automation methods have
/// failing defaults so a host can implement them one at a time.
pub trait Host {
    fn perform(&self, command: Command, cx: &mut App);

    fn snapshot(&self, _cx: &App) -> Snapshot {
        Snapshot::default()
    }

    fn create(&self, _place: Place, _config: SurfaceConfig, _cx: &mut App) -> Result<Created, String> {
        Err(UNSUPPORTED.into())
    }

    fn focus_terminal(&self, _id: &str, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn close_terminal(&self, _id: &str, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn activate_window(&self, _id: &str, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn close_window(&self, _id: &str, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn select_tab(&self, _id: &str, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn close_tab(&self, _id: &str, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn set_title_override(&self, _target: TitleTarget<'_>, _title: Option<String>, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    /// Runs a keybind action string in the window that holds `terminal`.
    fn perform_action(&self, _terminal: &str, _action: &str, _cx: &mut App) -> bool {
        false
    }

    fn input(&self, _terminal: &str, _input: Input, _cx: &mut App) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn details(&self, _terminal: &str, _detail: Detail, _cx: &mut App) -> Result<String, String> {
        Err(UNSUPPORTED.into())
    }

    /// Shows the quick terminal if hidden and returns the id of its terminal.
    fn open_quick_terminal(&self, _cx: &mut App) -> Result<Id, String> {
        Err(UNSUPPORTED.into())
    }

    /// The palette entries Shortcuts can invoke.
    fn commands(&self, _cx: &App) -> Vec<CommandInfo> {
        Vec::new()
    }
}

pub(crate) struct HostGlobal(pub Rc<dyn Host>);

impl Global for HostGlobal {}

pub(crate) fn host(cx: &App) -> Option<Rc<dyn Host>> {
    cx.try_global::<HostGlobal>().map(|global| global.0.clone())
}

pub fn perform(command: Command, cx: &mut App) {
    let Some(host) = cx.try_global::<HostGlobal>().map(|g| g.0.clone()) else {
        return;
    };
    host.perform(command, cx);
}
