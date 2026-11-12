//! The workspace's side of the panels: what it tells them about the window
//! and how it carries out what they ask for.

use gpui_kit::*;

use angelly_core::config::Action as ConfigAction;

use super::{ClipboardRequest, HostInfo, OverlayEvent, Overlays, TabEntry};
use crate::keybinds::Command;
use crate::tab_colors::display_color;
use crate::theme::Theme;
use crate::workspace::Workspace;

impl Workspace {
    /// The window's panel layer, created on first use.
    pub(crate) fn overlays(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Overlays> {
        if let Some(overlays) = &self.overlays {
            return overlays.clone();
        }
        let overlays = cx.new(|cx| Overlays::new(window, cx));
        cx.subscribe_in(&overlays, window, Self::on_overlay_event).detach();
        cx.observe(&overlays, |_, _, cx| cx.notify()).detach();
        self.overlays = Some(overlays.clone());
        overlays
    }

    /// Shows a program's clipboard request in the window's clipboard sheet.
    pub(crate) fn show_clipboard_request(&mut self, request: ClipboardRequest, window: &mut Window, cx: &mut Context<Self>) {
        let overlays = self.overlays(window, cx);
        overlays.update(cx, |overlays, cx| overlays.clipboard(request, window, cx));
    }

    /// Shows the "New Worktree" popover, anchored at `anchor` in window coordinates.
    pub(crate) fn show_new_worktree(&mut self, anchor: Option<Point<Pixels>>, window: &mut Window, cx: &mut Context<Self>) {
        let info = self.host_info(cx);
        let overlays = self.overlays(window, cx);
        overlays.update(cx, |overlays, cx| overlays.new_worktree(info, anchor, window, cx));
    }

    /// Shows the "Manage Worktrees" sheet.
    pub(crate) fn show_manage_worktrees(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let info = self.host_info(cx);
        let overlays = self.overlays(window, cx);
        overlays.update(cx, |overlays, cx| overlays.manage_worktrees(info, window, cx));
    }

    pub(crate) fn host_info(&self, cx: &App) -> HostInfo {
        let dark = cx.global::<Theme>().dark;
        let tabs = self
            .tabs
            .iter()
            .map(|tab| TabEntry {
                title: self.tab_title(tab, cx),
                pwd: tab.view.read(cx).cwd().map(|dir| dir.display().to_string()),
                color: display_color(tab.color, dark),
            })
            .collect();
        let terminal_dirs = self
            .tabs
            .iter()
            .flat_map(|tab| tab.splits.panes())
            .filter_map(|pane| pane.read(cx).cwd())
            .collect();
        HostInfo { tabs, working_dir: self.selected_view().read(cx).cwd(), terminal_dirs }
    }

    fn on_overlay_event(&mut self, _: &Entity<Overlays>, event: &OverlayEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            OverlayEvent::FocusTab(index) => self.select(*index, window, cx),
            OverlayEvent::OpenWorktree(path) => self.add_tab(Some(path.clone()), window, cx),
            OverlayEvent::Binding(text) => self.perform_binding(text, window, cx),
        }
    }

    fn perform_binding(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(action) = ConfigAction::parse(text) else { return };
        let Some(command) = Command::parse(&action) else { return };
        self.selected_view().clone().update(cx, |view, cx| {
            view.run_command(command, &action, window, cx);
        });
    }
}
