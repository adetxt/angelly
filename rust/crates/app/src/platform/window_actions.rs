//! Window-level `Perform` actions that no workspace handles itself: the ones about the
//! application as a whole or about the operating system's window management.

use gpui_kit::*;

use super::{command, float_on_top, menus, secure_input, visibility, Command};
use crate::keybinds::{AppAction, Direction, Perform, SplitDirection, SplitFocus};
use crate::splits::{
    ClosePane, EqualizeSplits, GotoSplitDown, GotoSplitLeft, GotoSplitNext, GotoSplitPrevious, GotoSplitRight, GotoSplitUp,
    ResizeSplitDown, ResizeSplitLeft, ResizeSplitRight, ResizeSplitUp, SplitDown, SplitLeft, SplitRight, SplitUp,
    ToggleSplitZoom,
};

pub fn register(cx: &mut App) {
    cx.on_action(|Perform(action): &Perform, cx| match action {
        AppAction::NewWindow => command::perform(Command::NewWindow, cx),
        AppAction::CloseAllWindows => command::perform(Command::CloseAllWindows, cx),
        AppAction::ReloadConfig => command::perform(Command::ReloadConfig, cx),
        AppAction::ToggleFullscreen => menus::with_active_window(cx, |window| window.toggle_fullscreen()),
        AppAction::ToggleMaximize => menus::with_active_window(cx, |window| window.zoom_window()),
        AppAction::ToggleWindowFloatOnTop => float_on_top::toggle(cx),
        AppAction::ToggleSecureInput => {
            secure_input::toggle(cx);
            menus::install(cx);
        }
        AppAction::ToggleQuickTerminal => command::perform(Command::ToggleQuickTerminal, cx),
        AppAction::ToggleVisibility => visibility::toggle(cx),
        AppAction::GotoWindow { next } => goto_window(*next, cx),
        other => {
            if let Some(split) = split_action(other) {
                dispatch_to_active_window(split, cx);
            }
        }
    });
}

/// The split actions of the menu bar behind the keybind form of the same operations.
pub(super) fn split_action(action: &AppAction) -> Option<Box<dyn Action>> {
    Some(match action {
        AppAction::NewSplit(SplitDirection::Right | SplitDirection::Auto) => Box::new(SplitRight),
        AppAction::NewSplit(SplitDirection::Left) => Box::new(SplitLeft),
        AppAction::NewSplit(SplitDirection::Down) => Box::new(SplitDown),
        AppAction::NewSplit(SplitDirection::Up) => Box::new(SplitUp),
        AppAction::GotoSplit(SplitFocus::Previous) => Box::new(GotoSplitPrevious),
        AppAction::GotoSplit(SplitFocus::Next) => Box::new(GotoSplitNext),
        AppAction::GotoSplit(SplitFocus::Up) => Box::new(GotoSplitUp),
        AppAction::GotoSplit(SplitFocus::Down) => Box::new(GotoSplitDown),
        AppAction::GotoSplit(SplitFocus::Left) => Box::new(GotoSplitLeft),
        AppAction::GotoSplit(SplitFocus::Right) => Box::new(GotoSplitRight),
        AppAction::ResizeSplit(Direction::Up, _) => Box::new(ResizeSplitUp),
        AppAction::ResizeSplit(Direction::Down, _) => Box::new(ResizeSplitDown),
        AppAction::ResizeSplit(Direction::Left, _) => Box::new(ResizeSplitLeft),
        AppAction::ResizeSplit(Direction::Right, _) => Box::new(ResizeSplitRight),
        AppAction::EqualizeSplits => Box::new(EqualizeSplits),
        AppAction::ToggleSplitZoom => Box::new(ToggleSplitZoom),
        AppAction::CloseSurface => Box::new(ClosePane),
        _ => return None,
    })
}

fn dispatch_to_active_window(action: Box<dyn Action>, cx: &mut App) {
    if let Some(handle) = cx.active_window() {
        let _ = handle.update(cx, |_, window, cx| window.dispatch_action(action, cx));
    }
}

/// Cycles the window stack: `next` brings the window behind the front one forward, the
/// other direction brings the rearmost one forward.
fn goto_window(next: bool, cx: &mut App) {
    let Some(stack) = cx.window_stack().filter(|stack| stack.len() > 1) else { return };
    let target = if next { stack[1] } else { stack[stack.len() - 1] };
    let _ = target.update(cx, |_, window, _| window.activate_window());
}
