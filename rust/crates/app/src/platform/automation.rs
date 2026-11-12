//! The automation model shared by AppleScript, Shortcuts and the command-line
//! client: a snapshot of windows, tabs and terminals, and a small JSON request
//! protocol for acting on them.
//!
//! Native bridges (the Swift shim on macOS, the socket server elsewhere) only
//! translate their own object model to and from these types; everything that touches
//! the application goes through [`Host`](super::Host).

use std::path::PathBuf;

use gpui_kit::App;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::command::{Direction, Host};

/// Stable identifier of a window, tab or terminal, unique for the life of the process.
pub type Id = String;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Snapshot {
    pub name: String,
    pub version: String,
    pub frontmost: bool,
    pub windows: Vec<WindowInfo>,
    pub terminals: Vec<TerminalInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WindowInfo {
    pub id: Id,
    pub title: String,
    pub selected_tab: Option<Id>,
    pub tabs: Vec<TabInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TabInfo {
    pub id: Id,
    pub title: String,
    pub title_override: Option<String>,
    pub index: usize,
    pub selected: bool,
    pub focused_terminal: Option<Id>,
    pub terminals: Vec<Id>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalKind {
    #[default]
    Normal,
    Quick,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TerminalInfo {
    pub id: Id,
    pub title: String,
    pub title_override: Option<String>,
    pub working_directory: Option<String>,
    pub pid: Option<u32>,
    pub tty: Option<String>,
    pub kind: TerminalKind,
    pub window: Option<Id>,
    pub tab: Option<Id>,
}

/// Settings for the terminal a command creates (the AppleScript `surface configuration`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SurfaceConfig {
    pub font_size: Option<f32>,
    pub working_directory: Option<PathBuf>,
    pub command: Option<String>,
    pub initial_input: Option<String>,
    pub wait_after_command: bool,
    pub environment: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    Window,
    Tab { window: Option<Id> },
    Split { terminal: Id, direction: Direction },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Created {
    pub window: Option<Id>,
    pub tab: Option<Id>,
    pub terminal: Option<Id>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Detail {
    Title,
    WorkingDirectory,
    AllContents,
    SelectedText,
    VisibleText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Press {
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub option: bool,
    pub command: bool,
}

impl Modifiers {
    /// Parses the AppleScript form: comma-separated `shift, control, option, command`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut mods = Self::default();
        for part in text.split(',').map(|p| p.trim().to_ascii_lowercase()).filter(|p| !p.is_empty()) {
            match part.as_str() {
                "shift" => mods.shift = true,
                "control" | "ctrl" => mods.control = true,
                "option" | "alt" => mods.option = true,
                "command" | "cmd" | "super" => mods.command = true,
                _ => return Err(format!("Unknown modifier in: {text}")),
            }
        }
        Ok(mods)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Momentum {
    #[default]
    None,
    Began,
    Changed,
    Ended,
    Cancelled,
    MayBegin,
    Stationary,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Text(String),
    Key { key: String, press: Press, mods: Modifiers },
    MouseButton { button: Button, press: Press, mods: Modifiers },
    MousePosition { x: f64, y: f64, mods: Modifiers },
    MouseScroll { x: f64, y: f64, precision: bool, momentum: Momentum },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandInfo {
    pub title: String,
    pub description: String,
    pub action: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleTarget<'a> {
    Tab(&'a str),
    Terminal(&'a str),
}

/// What an automation request can ask for, as it arrives over the bridge.
#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    NewWindow {
        #[serde(default)]
        config: SurfaceConfig,
    },
    NewTab {
        window: Option<Id>,
        #[serde(default)]
        config: SurfaceConfig,
    },
    Split {
        terminal: Id,
        direction: String,
        #[serde(default)]
        config: SurfaceConfig,
    },
    Focus { terminal: Id },
    Close { terminal: Id },
    ActivateWindow { window: Id },
    CloseWindow { window: Id },
    SelectTab { tab: Id },
    CloseTab { tab: Id },
    SetTitleOverride { tab: Option<Id>, terminal: Option<Id>, title: String },
    PerformAction { terminal: Id, action: String },
    InputText { terminal: Id, text: String },
    SendKey {
        terminal: Id,
        key: String,
        #[serde(default = "press_default")]
        action: Press,
        #[serde(default)]
        modifiers: String,
    },
    SendMouseButton {
        terminal: Id,
        button: Button,
        #[serde(default = "press_default")]
        action: Press,
        #[serde(default)]
        modifiers: String,
    },
    SendMousePosition {
        terminal: Id,
        x: f64,
        y: f64,
        #[serde(default)]
        modifiers: String,
    },
    SendMouseScroll {
        terminal: Id,
        x: f64,
        y: f64,
        #[serde(default)]
        precision: bool,
        #[serde(default)]
        momentum: Momentum,
    },
    Details { terminal: Id, detail: Detail },
    QuickTerminal,
    Commands,
    Snapshot,
}

fn press_default() -> Press {
    Press::Press
}

fn fail(message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": message.into() })
}

fn created(created: Created) -> Value {
    json!({ "ok": true, "window": created.window, "tab": created.tab, "terminal": created.terminal })
}

fn done(result: Result<(), String>) -> Value {
    match result {
        Ok(()) => json!({ "ok": true }),
        Err(message) => fail(message),
    }
}

/// Executes one request and returns the JSON response: `{"ok": true, ...}` or
/// `{"ok": false, "error": "..."}`.
pub fn handle(host: &dyn Host, request: Request, cx: &mut App) -> Value {
    match request {
        Request::NewWindow { config } => match host.create(Place::Window, config, cx) {
            Ok(result) => created(result),
            Err(message) => fail(message),
        },
        Request::NewTab { window, config } => match host.create(Place::Tab { window }, config, cx) {
            Ok(result) => created(result),
            Err(message) => fail(message),
        },
        Request::Split { terminal, direction, config } => match Direction::parse(&direction) {
            Some(direction) => match host.create(Place::Split { terminal, direction }, config, cx) {
                Ok(result) => created(result),
                Err(message) => fail(message),
            },
            None => fail(format!("Unknown split direction: {direction}")),
        },
        Request::Focus { terminal } => done(host.focus_terminal(&terminal, cx)),
        Request::Close { terminal } => done(host.close_terminal(&terminal, cx)),
        Request::ActivateWindow { window } => done(host.activate_window(&window, cx)),
        Request::CloseWindow { window } => done(host.close_window(&window, cx)),
        Request::SelectTab { tab } => done(host.select_tab(&tab, cx)),
        Request::CloseTab { tab } => done(host.close_tab(&tab, cx)),
        Request::SetTitleOverride { tab, terminal, title } => {
            let target = match (&tab, &terminal) {
                (Some(tab), _) => TitleTarget::Tab(tab),
                (None, Some(terminal)) => TitleTarget::Terminal(terminal),
                (None, None) => return fail("Missing title target."),
            };
            let title = (!title.is_empty()).then_some(title);
            done(host.set_title_override(target, title, cx))
        }
        Request::PerformAction { terminal, action } => {
            json!({ "ok": true, "result": host.perform_action(&terminal, &action, cx) })
        }
        Request::InputText { terminal, text } => done(host.input(&terminal, Input::Text(text), cx)),
        Request::SendKey { terminal, key, action, modifiers } => match Modifiers::parse(&modifiers) {
            Ok(mods) => done(host.input(&terminal, Input::Key { key, press: action, mods }, cx)),
            Err(message) => fail(message),
        },
        Request::SendMouseButton { terminal, button, action, modifiers } => match Modifiers::parse(&modifiers) {
            Ok(mods) => done(host.input(&terminal, Input::MouseButton { button, press: action, mods }, cx)),
            Err(message) => fail(message),
        },
        Request::SendMousePosition { terminal, x, y, modifiers } => match Modifiers::parse(&modifiers) {
            Ok(mods) => done(host.input(&terminal, Input::MousePosition { x, y, mods }, cx)),
            Err(message) => fail(message),
        },
        Request::SendMouseScroll { terminal, x, y, precision, momentum } => {
            done(host.input(&terminal, Input::MouseScroll { x, y, precision, momentum }, cx))
        }
        Request::Details { terminal, detail } => match host.details(&terminal, detail, cx) {
            Ok(text) => json!({ "ok": true, "text": text }),
            Err(message) => fail(message),
        },
        Request::QuickTerminal => match host.open_quick_terminal(cx) {
            Ok(terminal) => json!({ "ok": true, "terminal": terminal }),
            Err(message) => fail(message),
        },
        Request::Commands => json!({ "ok": true, "commands": host.commands(cx) }),
        Request::Snapshot => json!({ "ok": true, "snapshot": host.snapshot(cx) }),
    }
}

/// Parses and executes a JSON request string. Malformed input is reported as an error
/// response rather than panicking, since it comes from other processes.
pub fn handle_json(host: &dyn Host, request: &str, cx: &mut App) -> String {
    let response = match serde_json::from_str::<Request>(request) {
        Ok(request) => handle(host, request, cx),
        Err(error) => fail(format!("Invalid request: {error}")),
    };
    response.to_string()
}

pub fn snapshot_json(host: &dyn Host, cx: &App) -> String {
    serde_json::to_string(&host.snapshot(cx)).unwrap_or_else(|_| "{}".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_parse_the_applescript_form() {
        let mods = Modifiers::parse("shift, Control ,option").unwrap();
        assert!(mods.shift && mods.control && mods.option && !mods.command);
        assert_eq!(Modifiers::parse("").unwrap(), Modifiers::default());
        assert!(Modifiers::parse("shift, hyper").unwrap_err().contains("Unknown modifier"));
    }

    #[test]
    fn requests_deserialize_with_defaults() {
        let request: Request = serde_json::from_str(r#"{"cmd":"send_key","terminal":"t","key":"enter"}"#).unwrap();
        match request {
            Request::SendKey { action, modifiers, .. } => {
                assert_eq!(action, Press::Press);
                assert!(modifiers.is_empty());
            }
            other => panic!("unexpected {other:?}"),
        }
        let request: Request =
            serde_json::from_str(r#"{"cmd":"new_window","config":{"workingDirectory":"/tmp","waitAfterCommand":true}}"#).unwrap();
        match request {
            Request::NewWindow { config } => {
                assert_eq!(config.working_directory, Some(PathBuf::from("/tmp")));
                assert!(config.wait_after_command);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
