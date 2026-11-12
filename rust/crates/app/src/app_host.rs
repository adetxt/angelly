//! The application side of the platform layer: maps [`Command`]s and automation requests
//! onto windows, tabs and terminals.
//!
//! Windows are found through gpui's window stack, so index 0 is always the frontmost one.
//! Identifiers are derived from gpui ids and stay unique for the life of the process.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use angelly_core::config::Action as ConfigAction;
use angelly_core::persist::split_tree::NewDirection;
use gpui_kit::base::Root;
use gpui_kit::*;
use terminal::{Key, KeyAction, KeyEvent, MouseAction, MouseButton, MouseEvent, Mods, Wheel};

use crate::keybinds::{AppAction, Perform};
use crate::platform::automation::{
    Button, CommandInfo, Created, Detail, Id, Input, Modifiers, Momentum, Place, Press, Snapshot, SurfaceConfig,
    TabInfo, TerminalInfo, TerminalKind, TitleTarget, WindowInfo,
};
use crate::platform::command::Direction;
use crate::platform::{actions, float_on_top, open, Command, Host};
use crate::terminal_view::{stage_surface, unstage_surface, Surface, TerminalView};
use crate::theme::Theme;
use crate::workspace::{TabId, Workspace};
use crate::{panels, quick_terminal, session, workspace_files};

/// Offset of a new window from the one it opens next to.
const CASCADE: f32 = 24.0;

pub fn install(cx: &mut App) {
    crate::platform::init(cx, Rc::new(AppHost::default()));
    #[cfg(target_os = "macos")]
    install_updater(cx);
}

/// Hands the update controller an installer that swaps the app bundle, when the app runs
/// from one. The installer asks to quit through a channel because it works on a thread.
#[cfg(target_os = "macos")]
fn install_updater(cx: &mut App) {
    let Some(bundle) = crate::platform::updater::Installer::running_bundle() else { return };
    let (quit, requests) = async_channel::unbounded::<()>();
    cx.spawn(async move |cx| {
        while requests.recv().await.is_ok() {
            cx.update(quit_now);
        }
    })
    .detach();
    let installer = crate::platform::updater::Installer::new(bundle, move || {
        let _ = quit.try_send(());
    });
    panels::set_update_installer(std::sync::Arc::new(installer), cx);
}

/// Quits without asking, saving the front window's session first.
#[cfg(target_os = "macos")]
fn quit_now(cx: &mut App) {
    match windows(cx).into_iter().next() {
        Some(win) => {
            let _ = win.update(cx, |workspace, _, cx| workspace.quit(cx));
        }
        None => cx.quit(),
    }
}

#[derive(Default)]
struct AppHost {
    /// The last pointer position sent to each terminal, which button events reuse.
    pointers: RefCell<HashMap<EntityId, (f32, f32)>>,
}

#[derive(Clone)]
pub(crate) struct Win {
    pub(crate) handle: AnyWindowHandle,
    pub(crate) workspace: Entity<Workspace>,
}

impl Win {
    fn id(&self) -> Id {
        format!("window-{}", self.handle.window_id().as_u64())
    }

    fn tab_id(&self, tab: TabId) -> Id {
        format!("tab-{}-{tab}", self.handle.window_id().as_u64())
    }

    pub(crate) fn update<R>(
        &self,
        cx: &mut App,
        f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>) -> R,
    ) -> Result<R, String> {
        let workspace = self.workspace.clone();
        self.handle
            .update(cx, move |_, window, cx| workspace.update(cx, |workspace, cx| f(workspace, window, cx)))
            .map_err(|error| error.to_string())
    }
}

/// A terminal located in one of the workspace windows.
struct Found {
    win: Win,
    tab: usize,
    view: Entity<TerminalView>,
}

fn terminal_id(view: &Entity<TerminalView>) -> Id {
    format!("terminal-{}", view.entity_id().as_u64())
}

pub(crate) fn windows(cx: &App) -> Vec<Win> {
    let handles = cx.window_stack().unwrap_or_else(|| cx.windows());
    handles
        .into_iter()
        .filter_map(|handle| {
            let root = handle.downcast::<Root>()?;
            let workspace = root.read(cx).ok()?.view().clone().downcast::<Workspace>().ok()?;
            Some(Win { handle, workspace })
        })
        .collect()
}

fn find_window(id: &str, cx: &App) -> Result<Win, String> {
    windows(cx).into_iter().find(|win| win.id() == id).ok_or_else(|| format!("Window not found: {id}"))
}

fn find_terminal(id: &str, cx: &App) -> Result<Found, String> {
    for win in windows(cx) {
        let workspace = win.workspace.read(cx);
        for (tab, entry) in workspace.tabs.iter().enumerate() {
            if let Some(view) = entry.splits.panes().into_iter().find(|pane| terminal_id(pane) == id) {
                return Ok(Found { win: win.clone(), tab, view });
            }
        }
    }
    Err(format!("The terminal no longer exists: {id}"))
}

fn find_tab(id: &str, cx: &App) -> Result<(Win, usize), String> {
    for win in windows(cx) {
        let workspace = win.workspace.read(cx);
        if let Some(index) = workspace.tabs.iter().position(|tab| win.tab_id(tab.id) == id) {
            return Ok((win.clone(), index));
        }
    }
    Err(format!("Tab not found: {id}"))
}

/// Any terminal, including the quick terminal, for operations that only need its view.
fn find_view(id: &str, cx: &App) -> Result<Entity<TerminalView>, String> {
    if let Some(view) = quick_terminal::view(cx).filter(|view| terminal_id(view) == id) {
        return Ok(view);
    }
    find_terminal(id, cx).map(|found| found.view)
}

fn inherited_directory(cx: &App) -> Option<std::path::PathBuf> {
    let inherit = cx
        .global::<Theme>()
        .config
        .get("window-inherit-working-directory")
        .is_none_or(|value| value != "false");
    let front = windows(cx).into_iter().next().filter(|_| inherit)?;
    front.workspace.read(cx).selected_view().read(cx).cwd()
}

/// The font size a new window takes from the front one, unless `window-inherit-font-size` is off.
fn inherited_font_size(cx: &App) -> Option<f32> {
    let enabled = cx.global::<Theme>().config.get("window-inherit-font-size").is_none_or(|value| value != "false");
    let front = windows(cx).into_iter().next().filter(|_| enabled)?;
    Some(front.workspace.read(cx).selected_view().read(cx).font_size)
}

pub(crate) fn open_window(config: &SurfaceConfig, cx: &mut App) -> Result<Win, String> {
    let bounds = cx
        .active_window()
        .and_then(|handle| handle.update(cx, |_, window, _| window.bounds()).ok())
        .map(|bounds| Bounds::new(bounds.origin + point(px(CASCADE), px(CASCADE)), bounds.size));
    let cwd = config.working_directory.clone();
    let surface = Surface::from(config);
    let font_size = inherited_font_size(cx);
    let (handle, workspace) = session::open_window(bounds, cx, move |window, cx| {
        cx.new(|cx| {
            let mut workspace = Workspace::empty(cx);
            stage_surface(cx, surface);
            workspace.add_tab(cwd, window, cx);
            if let Some(size) = font_size {
                workspace.selected_view().update(cx, |view, cx| view.set_font_size(size, cx));
            }
            unstage_surface(cx);
            workspace.start_git_polling(cx);
            workspace.install_window_hooks(window, cx);
            workspace
        })
    })
    .map_err(|error| error.to_string())?;
    cx.activate(true);
    Ok(Win { handle, workspace })
}

fn close_windows(cx: &mut App) {
    for win in windows(cx) {
        let _ = win.update(cx, |workspace, window, cx| workspace.close_window(window, cx));
    }
}

/// Closes every terminal window, asking once when any of them runs a command.
fn close_all_windows(cx: &mut App) {
    let all = windows(cx);
    let busy = all.iter().any(|win| win.workspace.read(cx).needs_confirm_quit(cx));
    let Some(front) = all.into_iter().next().filter(|_| busy) else { return close_windows(cx) };
    let _ = front.update(cx, move |workspace, window, cx| {
        let request = panels::Confirm::close_all_windows(|_, cx| cx.defer(close_windows));
        workspace.ask(request, window, cx);
    });
}

fn modifiers(mods: Modifiers) -> Mods {
    let mut result = Mods::default();
    for (set, flag) in [(mods.shift, Mods::SHIFT), (mods.control, Mods::CTRL), (mods.option, Mods::ALT), (mods.command, Mods::SUPER)] {
        if set {
            result |= flag;
        }
    }
    result
}

/// Maps the key names scripts use (`a`, `enter`, `arrow_up`, `f5`, ...) to terminal keys.
fn parse_key(name: &str) -> Option<Key> {
    let normalized: String = name.chars().filter(|c| !matches!(c, '_' | '-' | ' ')).collect::<String>().to_lowercase();
    let key = match normalized.as_str() {
        "a" | "keya" => Key::A,
        "b" | "keyb" => Key::B,
        "c" | "keyc" => Key::C,
        "d" | "keyd" => Key::D,
        "e" | "keye" => Key::E,
        "f" | "keyf" => Key::F,
        "g" | "keyg" => Key::G,
        "h" | "keyh" => Key::H,
        "i" | "keyi" => Key::I,
        "j" | "keyj" => Key::J,
        "k" | "keyk" => Key::K,
        "l" | "keyl" => Key::L,
        "m" | "keym" => Key::M,
        "n" | "keyn" => Key::N,
        "o" | "keyo" => Key::O,
        "p" | "keyp" => Key::P,
        "q" | "keyq" => Key::Q,
        "r" | "keyr" => Key::R,
        "s" | "keys" => Key::S,
        "t" | "keyt" => Key::T,
        "u" | "keyu" => Key::U,
        "v" | "keyv" => Key::V,
        "w" | "keyw" => Key::W,
        "x" | "keyx" => Key::X,
        "y" | "keyy" => Key::Y,
        "z" | "keyz" => Key::Z,
        "0" | "digit0" | "zero" => Key::Digit0,
        "1" | "digit1" | "one" => Key::Digit1,
        "2" | "digit2" | "two" => Key::Digit2,
        "3" | "digit3" | "three" => Key::Digit3,
        "4" | "digit4" | "four" => Key::Digit4,
        "5" | "digit5" | "five" => Key::Digit5,
        "6" | "digit6" | "six" => Key::Digit6,
        "7" | "digit7" | "seven" => Key::Digit7,
        "8" | "digit8" | "eight" => Key::Digit8,
        "9" | "digit9" | "nine" => Key::Digit9,
        "enter" | "return" => Key::Enter,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "escape" | "esc" => Key::Escape,
        "backspace" => Key::Backspace,
        "delete" | "forwarddelete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "arrowup" | "up" => Key::ArrowUp,
        "arrowdown" | "down" => Key::ArrowDown,
        "arrowleft" | "left" => Key::ArrowLeft,
        "arrowright" | "right" => Key::ArrowRight,
        "minus" => Key::Minus,
        "equal" => Key::Equal,
        "comma" => Key::Comma,
        "period" => Key::Period,
        "slash" => Key::Slash,
        "backslash" => Key::Backslash,
        "semicolon" => Key::Semicolon,
        "quote" => Key::Quote,
        "backquote" | "grave" => Key::Backquote,
        "bracketleft" => Key::BracketLeft,
        "bracketright" => Key::BracketRight,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        _ => return None,
    };
    Some(key)
}

fn print_text(key: &str, mods: Modifiers) -> Option<String> {
    let mut chars = key.chars();
    let (Some(first), None) = (chars.next(), chars.next()) else { return None };
    if mods.control || mods.option || mods.command {
        return None;
    }
    Some(if mods.shift { first.to_uppercase().collect() } else { first.to_string() })
}

impl AppHost {
    fn front(&self, cx: &App) -> Option<Win> {
        windows(cx).into_iter().next()
    }

    fn send(&self, view: &Entity<TerminalView>, bytes: Vec<u8>, cx: &mut App) {
        if bytes.is_empty() {
            return;
        }
        view.update(cx, |view, cx| {
            if !view.is_read_only() {
                view.session().write(&bytes);
                view.note_input(cx);
            }
        });
    }

    fn focus(&self, found: &Found, cx: &mut App) -> Result<(), String> {
        let view = found.view.clone();
        let tab = found.tab;
        found.win.update(cx, move |workspace, window, cx| {
            workspace.select(tab, window, cx);
            workspace.pane_focused(&view, cx);
            let focus = view.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            window.activate_window();
        })
    }

    fn created(&self, win: &Win, view: &Entity<TerminalView>, cx: &App) -> Created {
        let workspace = win.workspace.read(cx);
        let tab = workspace.tabs.iter().find(|tab| tab.splits.contains(view)).map(|tab| win.tab_id(tab.id));
        Created { window: Some(win.id()), tab, terminal: Some(terminal_id(view)) }
    }

    fn new_tab(&self, win: &Win, config: &SurfaceConfig, cx: &mut App) -> Result<Created, String> {
        let cwd = config.working_directory.clone();
        let surface = Surface::from(config);
        let view = win.update(cx, move |workspace, window, cx| {
            stage_surface(cx, surface);
            workspace.add_tab(cwd, window, cx);
            unstage_surface(cx);
            workspace.selected_view().clone()
        })?;
        Ok(self.created(win, &view, cx))
    }

    fn surface_action(&self, text: &str, cx: &mut App) -> bool {
        match cx.active_window() {
            Some(handle) => actions::perform_in_window(handle, text, cx),
            None => false,
        }
    }

    fn all_text(&self, view: &Entity<TerminalView>, cx: &App, visible: bool) -> String {
        let mut term = view.read(cx).session().term();
        let had_selection = term.has_selection();
        let selected = if visible {
            let size = term.size();
            term.select_range((0, 0), (size.cols.saturating_sub(1), size.rows.saturating_sub(1)), false)
        } else {
            term.select_all()
        };
        let text = if selected { term.selection_text().unwrap_or_default() } else { String::new() };
        if !had_selection {
            term.clear_selection();
        }
        text
    }
}

impl Host for AppHost {
    fn perform(&self, command: Command, cx: &mut App) {
        let front = self.front(cx);
        let dispatch = |action: AppAction, cx: &mut App| {
            if let Some(win) = &front {
                let _ = win.handle.update(cx, |_, window, cx| window.dispatch_action(Box::new(Perform(action)), cx));
            }
        };
        match command {
            Command::ToggleQuickTerminal => quick_terminal::toggle(cx),
            Command::Quit => match &front {
                Some(_) => dispatch(AppAction::Quit, cx),
                None => cx.quit(),
            },
            Command::ReloadConfig => panels::reload_theme(cx),
            Command::NewWindow => {
                let config = SurfaceConfig { working_directory: inherited_directory(cx), ..Default::default() };
                let _ = open_window(&config, cx);
            }
            Command::NewTab => match &front {
                Some(_) => dispatch(AppAction::NewTab, cx),
                None => drop(open_window(&SurfaceConfig::default(), cx)),
            },
            Command::OpenDirectories { paths, tabs } => {
                for path in paths {
                    let config = SurfaceConfig { working_directory: Some(path), ..Default::default() };
                    let _ = match self.front(cx).filter(|_| tabs) {
                        Some(win) => self.new_tab(&win, &config, cx).map(drop),
                        None => open_window(&config, cx).map(drop),
                    };
                }
            }
            Command::RunFile { path, tab } => {
                let config = SurfaceConfig {
                    working_directory: path.parent().map(ToOwned::to_owned),
                    initial_input: Some(open::run_input(&path)),
                    ..Default::default()
                };
                let _ = self.create(if tab { Place::Tab { window: None } } else { Place::Window }, config, cx);
            }
            Command::SaveWorkspace => workspace_files::save(cx),
            Command::OpenWorkspace => workspace_files::open(cx),
            Command::OpenRecentlyClosed(index) => session::reopen_closed_window(Some(index), cx),
            Command::ClearRecentlyClosed => session::clear_closed_windows(cx),
            Command::CloseTab => dispatch(AppAction::CloseTab(crate::keybinds::CloseTabMode::This), cx),
            Command::CloseWindow => dispatch(AppAction::CloseWindow, cx),
            Command::CloseAllWindows => close_all_windows(cx),
            Command::ToggleSidebar => {
                if let Some(win) = front {
                    let _ = win.update(cx, |workspace, _, cx| {
                        let visible = !workspace.sidebar_visible;
                        workspace.set_sidebar_visible(visible, cx);
                    });
                }
            }
            Command::ChangeTabTitle => dispatch(AppAction::PromptTitle(crate::keybinds::TitleTarget::Tab), cx),
            Command::ChangeTerminalTitle => dispatch(AppAction::PromptTitle(crate::keybinds::TitleTarget::Surface), cx),
            Command::ToggleInspector => dispatch(AppAction::Inspector(crate::keybinds::InspectorMode::Toggle), cx),
            Command::ReturnToDefaultSize => dispatch(AppAction::ResetWindowSize, cx),
            Command::ToggleFloatOnTop => float_on_top::toggle(cx),
            command => {
                if let Some(text) = surface_binding(&command) {
                    self.surface_action(text, cx);
                }
            }
        }
    }

    fn snapshot(&self, cx: &App) -> Snapshot {
        let mut snapshot = Snapshot {
            name: "Angelly".into(),
            version: crate::panels::about::AppInfo::current().version,
            frontmost: cx.active_window().is_some(),
            ..Snapshot::default()
        };
        let describe = |view: &Entity<TerminalView>, window: Option<&Id>, tab: Option<&Id>, kind: TerminalKind| {
            let view_ref = view.read(cx);
            TerminalInfo {
                id: terminal_id(view),
                title: view_ref.title(),
                title_override: view_ref.title_override.clone(),
                working_directory: view_ref.cwd().map(|dir| dir.to_string_lossy().into_owned()),
                pid: view_ref.session().pid(),
                tty: None,
                kind,
                window: window.cloned(),
                tab: tab.cloned(),
            }
        };
        for win in windows(cx) {
            let workspace = win.workspace.read(cx);
            let window_id = win.id();
            let mut tabs = Vec::new();
            for (index, tab) in workspace.tabs.iter().enumerate() {
                let tab_id = win.tab_id(tab.id);
                let panes = tab.splits.panes();
                for pane in &panes {
                    snapshot.terminals.push(describe(pane, Some(&window_id), Some(&tab_id), TerminalKind::Normal));
                }
                tabs.push(TabInfo {
                    id: tab_id,
                    title: workspace.tab_title(tab, cx),
                    title_override: tab.title_override.clone(),
                    index: index + 1,
                    selected: index == workspace.selected,
                    focused_terminal: Some(terminal_id(&tab.view)),
                    terminals: panes.iter().map(terminal_id).collect(),
                });
            }
            let selected = tabs.get(workspace.selected).cloned();
            snapshot.windows.push(WindowInfo {
                id: window_id,
                title: selected.as_ref().map(|tab| tab.title.clone()).unwrap_or_default(),
                selected_tab: selected.map(|tab| tab.id),
                tabs,
            });
        }
        if let Some(view) = quick_terminal::view(cx) {
            snapshot.terminals.push(describe(&view, None, None, TerminalKind::Quick));
        }
        snapshot
    }

    fn create(&self, place: Place, config: SurfaceConfig, cx: &mut App) -> Result<Created, String> {
        match place {
            Place::Window => {
                let win = open_window(&config, cx)?;
                let view = win.workspace.read(cx).selected_view().clone();
                Ok(self.created(&win, &view, cx))
            }
            Place::Tab { window } => {
                let win = match window {
                    Some(id) => find_window(&id, cx)?,
                    None => match self.front(cx) {
                        Some(win) => win,
                        None => return self.create(Place::Window, config, cx),
                    },
                };
                self.new_tab(&win, &config, cx)
            }
            Place::Split { terminal, direction } => {
                let found = find_terminal(&terminal, cx)?;
                self.focus(&found, cx)?;
                let direction = match direction {
                    Direction::Right => NewDirection::Right,
                    Direction::Left => NewDirection::Left,
                    Direction::Down => NewDirection::Down,
                    Direction::Up => NewDirection::Up,
                };
                let surface = Surface::from(&config);
                let view = found.win.update(cx, move |workspace, window, cx| {
                    stage_surface(cx, surface);
                    workspace.split_focused(direction, window, cx);
                    unstage_surface(cx);
                    workspace.tabs[workspace.selected].view.clone()
                })?;
                Ok(self.created(&found.win, &view, cx))
            }
        }
    }

    fn focus_terminal(&self, id: &str, cx: &mut App) -> Result<(), String> {
        let found = find_terminal(id, cx)?;
        self.focus(&found, cx)
    }

    fn close_terminal(&self, id: &str, cx: &mut App) -> Result<(), String> {
        let found = find_terminal(id, cx)?;
        let view = found.view.clone();
        found.win.update(cx, move |workspace, window, cx| workspace.close_pane(&view, window, cx))
    }

    fn activate_window(&self, id: &str, cx: &mut App) -> Result<(), String> {
        let win = find_window(id, cx)?;
        cx.activate(true);
        win.handle.update(cx, |_, window, _| window.activate_window()).map_err(|error| error.to_string())
    }

    fn close_window(&self, id: &str, cx: &mut App) -> Result<(), String> {
        find_window(id, cx)?.update(cx, |workspace, window, cx| workspace.request_close_window(window, cx))
    }

    fn select_tab(&self, id: &str, cx: &mut App) -> Result<(), String> {
        let (win, index) = find_tab(id, cx)?;
        win.update(cx, move |workspace, window, cx| {
            workspace.select(index, window, cx);
            window.activate_window();
        })
    }

    fn close_tab(&self, id: &str, cx: &mut App) -> Result<(), String> {
        let (win, index) = find_tab(id, cx)?;
        let tab = win.workspace.read(cx).tabs[index].id;
        win.update(cx, move |workspace, window, cx| workspace.request_close_tab(tab, window, cx))
    }

    fn set_title_override(&self, target: TitleTarget<'_>, title: Option<String>, cx: &mut App) -> Result<(), String> {
        match target {
            TitleTarget::Tab(id) => {
                let (win, index) = find_tab(id, cx)?;
                let tab = win.workspace.read(cx).tabs[index].id;
                let title = title.unwrap_or_default();
                win.update(cx, move |workspace, _, cx| workspace.set_title_override(tab, &title, cx))
            }
            TitleTarget::Terminal(id) => {
                let view = find_view(id, cx)?;
                view.update(cx, |view, cx| {
                    view.title_override = title;
                    cx.notify();
                });
                Ok(())
            }
        }
    }

    fn perform_action(&self, terminal: &str, action: &str, cx: &mut App) -> bool {
        let Ok(found) = find_terminal(terminal, cx) else { return false };
        if self.focus(&found, cx).is_err() || ConfigAction::parse(action).is_err() {
            return false;
        }
        actions::perform_in_window(found.win.handle, action, cx)
    }

    fn input(&self, terminal: &str, input: Input, cx: &mut App) -> Result<(), String> {
        let view = find_view(terminal, cx)?;
        let entity = view.entity_id();
        match input {
            Input::Text(text) => view.update(cx, |view, cx| view.send_text(&text, cx)),
            Input::Key { key, press, mods } => {
                let code = parse_key(&key).ok_or_else(|| format!("Unknown key: {key}"))?;
                let text = print_text(&key, mods).unwrap_or_default();
                let action = match press {
                    Press::Press => KeyAction::Press,
                    Press::Release => KeyAction::Release,
                };
                let event = KeyEvent::press(code, modifiers(mods)).with_action(action).with_text(&text);
                let bytes = view.read(cx).session().term().encode_key(&event);
                self.send(&view, bytes, cx);
            }
            Input::MouseButton { button, press, mods } => {
                let (x, y) = self.pointers.borrow().get(&entity).copied().unwrap_or_default();
                let event = MouseEvent {
                    action: match press {
                        Press::Press => MouseAction::Press,
                        Press::Release => MouseAction::Release,
                    },
                    button: Some(match button {
                        Button::Left => MouseButton::Left,
                        Button::Right => MouseButton::Right,
                        Button::Middle => MouseButton::Middle,
                    }),
                    mods: modifiers(mods),
                    x,
                    y,
                };
                let bytes = view.read(cx).session().term().encode_mouse(&event);
                self.send(&view, bytes, cx);
            }
            Input::MousePosition { x, y, mods } => {
                let (x, y) = (x as f32, y as f32);
                self.pointers.borrow_mut().insert(entity, (x, y));
                let event = MouseEvent { action: MouseAction::Motion, button: None, mods: modifiers(mods), x, y };
                let bytes = view.read(cx).session().term().encode_mouse(&event);
                self.send(&view, bytes, cx);
            }
            Input::MouseScroll { x: _, y, precision, momentum } => {
                if !matches!(momentum, Momentum::None | Momentum::Began | Momentum::Changed) {
                    return Ok(());
                }
                let (px_x, px_y) = self.pointers.borrow().get(&entity).copied().unwrap_or_default();
                let bytes = {
                    let mut term = view.read(cx).session().term();
                    let lines = if precision { y / f64::from(term.size().cell_height.max(1)) } else { y };
                    match term.wheel(lines.round() as i32, px_x, px_y, Mods::default()) {
                        Wheel::Scrolled => Vec::new(),
                        Wheel::Send(bytes) => bytes,
                    }
                };
                self.send(&view, bytes, cx);
                view.update(cx, |_, cx| cx.notify());
            }
        }
        Ok(())
    }

    fn details(&self, terminal: &str, detail: Detail, cx: &mut App) -> Result<String, String> {
        let view = find_view(terminal, cx)?;
        Ok(match detail {
            Detail::Title => view.read(cx).title(),
            Detail::WorkingDirectory => {
                view.read(cx).cwd().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default()
            }
            Detail::SelectedText => view.read(cx).session().term().selection_text().unwrap_or_default(),
            Detail::AllContents => self.all_text(&view, cx, false),
            Detail::VisibleText => self.all_text(&view, cx, true),
        })
    }

    fn open_quick_terminal(&self, cx: &mut App) -> Result<Id, String> {
        quick_terminal::show(cx)?;
        quick_terminal::view(cx).map(|view| terminal_id(&view)).ok_or_else(|| "The quick terminal is unavailable.".into())
    }

    fn commands(&self, cx: &App) -> Vec<CommandInfo> {
        panels::commands_for(&cx.global::<Theme>().config)
            .into_iter()
            .filter(|command| command.is_supported())
            .map(|command| CommandInfo { title: command.title, description: command.description, action: command.action })
            .collect()
    }
}

/// The keybind action behind a menu command that acts on the focused terminal.
fn surface_binding(command: &Command) -> Option<&'static str> {
    Some(match command {
        Command::Undo => "undo",
        Command::Redo => "redo",
        Command::PasteSelection => "paste_from_selection",
        Command::Find => "start_search",
        Command::FindNext => "navigate_search:next",
        Command::FindPrevious => "navigate_search:previous",
        Command::HideFindBar => "end_search",
        Command::UseSelectionForFind => "search_selection",
        Command::JumpToSelection => "scroll_to_selection",
        Command::ResetFontSize => "reset_font_size",
        Command::IncreaseFontSize => "increase_font_size:1",
        Command::DecreaseFontSize => "decrease_font_size:1",
        Command::ToggleReadonly => "toggle_readonly",
        Command::UseAsDefaultSize => "use_as_default_size",
        _ => return None,
    })
}
