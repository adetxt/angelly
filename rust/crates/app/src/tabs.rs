//! Tab management on the workspace: order, groups, colors, titles, closing with
//! confirmation, and the tab shortcuts.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use angelly_core::config::ConfirmCloseSurface;
use angelly_core::persist::{
    rows, tab_item_title, tab_title, Row, TabItem, TerminalTabColor,
};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use uuid::Uuid;

use crate::keybinds::{AppAction, CloseTabMode, Perform, TitleTarget};
use crate::panels::Confirm;
use crate::splits::Splits;
use crate::tab_ops::{self, Grouped};
use crate::chrome::{GroupRename, RowKey};
use crate::theme::Theme;
use crate::workspace::{Tab, TabId, Workspace};

actions!(tabs, [ReopenClosedWindow]);

/// Handles the tab, title and window-close actions of the Ghostty keybinds.
pub fn register(root: Div, cx: &mut Context<Workspace>) -> Div {
    root.on_action(cx.listener(|this, Perform(action): &Perform, window, cx| match action {
        AppAction::NewTab => this.new_tab_from_selected(window, cx),
        AppAction::PreviousTab => this.step_tab(-1, window, cx),
        AppAction::NextTab => this.step_tab(1, window, cx),
        AppAction::LastTab => this.goto_last_tab(window, cx),
        AppAction::GotoTab(number) => this.goto_tab(*number, window, cx),
        AppAction::MoveTab(offset) => this.move_selected_tab(*offset, cx),
        AppAction::PromptTitle(TitleTarget::Tab) => {
            let id = this.tabs[this.selected].id;
            this.prompt_rename_tab(id, window, cx);
        }
        AppAction::SetTitle(TitleTarget::Tab, title) => {
            let id = this.tabs[this.selected].id;
            this.set_title_override(id, title, cx);
        }
        AppAction::CloseTab(mode) => {
            let id = this.tabs[this.selected].id;
            let scope = match mode {
                CloseTabMode::This => CloseScope::Tab(id),
                CloseTabMode::Other => CloseScope::Others(id),
                CloseTabMode::Right => CloseScope::Right(id),
            };
            this.close_scope(scope, window, cx);
        }
        AppAction::MoveTabToNewWindow => this.move_tab_to_new_window(window, cx),
        AppAction::CloseWindow => this.request_close_window(window, cx),
        AppAction::Quit => this.request_quit(window, cx),
        _ => cx.propagate(),
    }))
}

impl Grouped for Tab {
    fn group(&self) -> Option<Uuid> {
        self.group
    }

    fn set_group(&mut self, group: Option<Uuid>) {
        self.group = group;
    }
}

/// What a close request removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseScope {
    Tab(TabId),
    Others(TabId),
    Right(TabId),
    Group(Uuid),
}

impl CloseScope {
    fn request(self, whole_window: bool, on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Confirm {
        if whole_window {
            return Confirm::new(
                "Close Window?",
                "All terminal sessions in this window will be terminated.",
                "Close",
                on_confirm,
            );
        }
        match self {
            CloseScope::Tab(_) => Confirm::close_tab(on_confirm),
            CloseScope::Others(_) => Confirm::close_other_tabs(on_confirm),
            CloseScope::Right(_) => Confirm::close_tabs_to_right(on_confirm),
            CloseScope::Group(_) => Confirm::close_group(on_confirm),
        }
    }
}

/// The tab label: the user's override, else the terminal title without a default
/// `user@host:` prefix, else "Terminal".
pub fn tab_label(override_title: Option<&str>, terminal_title: &str) -> String {
    match override_title {
        Some(title) => title.to_string(),
        None => tab_item_title(&tab_title(terminal_title, false)).to_string(),
    }
}

impl Workspace {
    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    pub fn next_tab_id(&mut self) -> TabId {
        self.chrome.next_id += 1;
        self.chrome.next_id
    }

    pub fn tab_title(&self, tab: &Tab, cx: &App) -> String {
        tab_label(tab.title_override.as_deref(), &tab.view.read(cx).title())
    }

    /// The tabs as the sidebar and tab bar list them.
    pub fn items(&self, cx: &App) -> Vec<TabItem<TabId>> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| {
                let info = tab.group.and_then(|g| self.chrome.groups.get(g).map(|info| (g, info)));
                TabItem {
                    id: tab.id,
                    title: self.tab_title(tab, cx),
                    color: Some(tab.color).filter(|c| info.is_none() && *c != TerminalTabColor::None),
                    shortcut: (index < 9).then(|| format!("⌘{}", index + 1)),
                    has_bell: tab.view.read(cx).has_bell(),
                    is_selected: index == self.selected,
                    group: info.map(|(g, _)| g),
                    group_color: info.map(|(_, i)| i.color).filter(|c| *c != TerminalTabColor::None),
                    git_status: self.chrome.git.status_for(tab.id).cloned(),
                }
            })
            .collect()
    }

    pub fn rows(&self, cx: &App) -> Vec<Row<TabId>> {
        rows(&self.items(cx), self.chrome.groups.groups())
    }

    // MARK: Order and groups

    /// Applies a reordering and keeps the selected tab selected.
    pub(crate) fn reorder<R>(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Vec<Tab>) -> R) -> R {
        let selected = self.tabs[self.selected].id;
        let result = change(&mut self.tabs);
        self.selected = self.tab_index(selected).unwrap_or(0);
        self.prune_groups();
        self.tabs_changed(cx);
        result
    }

    /// Drops groups no tab belongs to any more.
    pub fn prune_groups(&mut self) {
        let live: HashSet<Uuid> = self.tabs.iter().filter_map(|t| t.group).collect();
        self.chrome.groups.prune(&live);
    }

    /// Persists and repaints after the tab list or its metadata changed.
    pub fn tabs_changed(&mut self, cx: &mut Context<Self>) {
        self.save_session(cx);
        cx.notify();
    }

    /// Puts the tab in a new group and starts renaming it.
    pub fn new_group_with(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else { return };
        if self.tabs[index].group.is_some() {
            self.reorder(cx, |tabs| tab_ops::leave_group(tabs, index));
        }
        let group = self.chrome.groups.create();
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.group = Some(group);
        }
        self.begin_group_rename(group, window, cx);
        self.tabs_changed(cx);
    }

    pub fn add_to_group(&mut self, id: TabId, group: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else { return };
        self.reorder(cx, |tabs| tab_ops::join_group(tabs, index, group));
    }

    pub fn leave_group(&mut self, id: TabId, cx: &mut Context<Self>) {
        let Some(index) = self.tab_index(id) else { return };
        self.reorder(cx, |tabs| tab_ops::leave_group(tabs, index));
    }

    pub fn ungroup(&mut self, group: Uuid, cx: &mut Context<Self>) {
        tab_ops::ungroup(&mut self.tabs, group);
        self.chrome.groups.remove(group);
        self.tabs_changed(cx);
    }

    pub fn toggle_collapsed(&mut self, group: Uuid, cx: &mut Context<Self>) {
        self.chrome.groups.update(group, |info| info.is_collapsed = !info.is_collapsed);
        self.tabs_changed(cx);
    }

    pub fn set_group_color(&mut self, group: Uuid, color: TerminalTabColor, cx: &mut Context<Self>) {
        self.chrome.groups.update(group, |info| info.color = color);
        self.tabs_changed(cx);
    }

    pub fn set_tab_color(&mut self, id: TabId, color: TerminalTabColor, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.color = color;
        }
        self.tabs_changed(cx);
    }

    pub fn set_title_override(&mut self, id: TabId, title: &str, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.title_override = angelly_core::persist::normalize_title_override(title);
        }
        self.tabs_changed(cx);
    }

    // MARK: Selection

    pub fn goto_tab(&mut self, number: usize, window: &mut Window, cx: &mut Context<Self>) {
        if number >= 1 && number <= self.tabs.len() {
            self.select(number - 1, window, cx);
        }
    }

    pub fn goto_last_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.select(self.tabs.len() - 1, window, cx);
    }

    /// Where a new tab goes and which group it joins, from `window-new-tab-position`.
    pub fn new_tab_placement(&self, cx: &App) -> (usize, Option<Uuid>) {
        let position = cx.global::<Theme>().config.window_new_tab_position();
        tab_ops::new_tab_placement(&self.tabs, self.selected, position)
    }

    // MARK: Closing

    fn tab_needs_confirmation(&self, tab: &Tab, cx: &App) -> bool {
        match cx.global::<Theme>().config.confirm_close_surface() {
            ConfirmCloseSurface::False => false,
            ConfirmCloseSurface::Always => true,
            ConfirmCloseSurface::True => tab.splits.panes().iter().any(|pane| pane.read(cx).has_running_command()),
        }
    }

    pub fn needs_confirm_quit(&self, cx: &App) -> bool {
        self.tabs.iter().any(|tab| self.tab_needs_confirmation(tab, cx))
    }

    fn scope_ids(&self, scope: CloseScope) -> Vec<TabId> {
        match scope {
            CloseScope::Tab(id) => vec![id],
            CloseScope::Others(id) => self.tabs.iter().map(|t| t.id).filter(|t| *t != id).collect(),
            CloseScope::Right(id) => {
                let from = self.tab_index(id).map_or(self.tabs.len(), |i| i + 1);
                self.tabs[from..].iter().map(|t| t.id).collect()
            }
            CloseScope::Group(group) => self.tabs.iter().filter(|t| t.group == Some(group)).map(|t| t.id).collect(),
        }
    }

    /// Closes tabs, asking first when one of them runs a command.
    pub fn close_scope(&mut self, scope: CloseScope, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.scope_ids(scope);
        if ids.is_empty() {
            return;
        }
        let whole_window = ids.len() == self.tabs.len();
        let confirm = ids
            .iter()
            .filter_map(|id| self.tabs.iter().find(|t| t.id == *id))
            .any(|tab| self.tab_needs_confirmation(tab, cx));
        if !confirm {
            self.close_ids(&ids, window, cx);
            return;
        }
        let workspace = cx.entity().downgrade();
        let request = scope.request(whole_window, move |window, cx| {
            workspace.update(cx, |this, cx| this.close_ids(&ids, window, cx)).ok();
        });
        self.ask(request, window, cx);
    }

    /// Shows a question in the window's panel layer. A second question waits for the first.
    pub fn ask(&mut self, request: Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let overlays = self.overlays(window, cx);
        overlays.update(cx, |overlays, cx| {
            if !overlays.is_confirming() {
                overlays.confirm(request, window, cx);
            }
        });
    }

    /// Closes tabs without asking. Closing every tab closes the window.
    pub fn close_ids(&mut self, ids: &[TabId], window: &mut Window, cx: &mut Context<Self>) {
        if ids.len() >= self.tabs.len() && self.tabs.iter().all(|t| ids.contains(&t.id)) {
            self.close_window(window, cx);
            return;
        }
        for id in ids {
            if let Some(index) = self.tab_index(*id) {
                self.close_tab(index, window, cx);
            }
        }
    }

    /// Closes the window, keeping it in the recently closed windows. The last window
    /// closing quits the app when `quit-after-last-window-closed` asks for it.
    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hover_card.dismiss(cx);
        self.record_closed_window(cx);
        self.chrome.closing = true;
        let quits = cx.windows().len() <= 1 && cx.global::<Theme>().config.quit_after_last_window_closed();
        window.remove_window();
        if quits {
            cx.quit();
        }
    }

    /// Closes the window, asking first when a tab runs a command.
    pub fn request_close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.needs_confirm_quit(cx) {
            self.close_window(window, cx);
            return;
        }
        let workspace = cx.entity().downgrade();
        let request = Confirm::new(
            "Close Window?",
            "All terminal sessions in this window will be terminated.",
            "Close",
            move |window, cx| {
                workspace.update(cx, |this, cx| this.close_window(window, cx)).ok();
            },
        );
        self.ask(request, window, cx);
    }

    pub fn request_close_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        self.close_scope(CloseScope::Tab(id), window, cx);
    }

    /// Asks before quitting while a tab still runs a command.
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.needs_confirm_quit(cx) {
            self.quit(cx);
            return;
        }
        let workspace = cx.entity().downgrade();
        let request = Confirm::quit(move |_, cx| {
            workspace.update(cx, |this, cx| this.quit(cx)).ok();
        });
        self.ask(request, window, cx);
    }

    /// Quits, keeping the session of every window for the next launch.
    pub fn quit(&mut self, cx: &mut Context<Self>) {
        self.write_session(cx);
        for win in crate::app_host::windows(cx) {
            win.workspace.read(cx).write_session(cx);
        }
        self.chrome.closing = true;
        cx.quit();
    }

    // MARK: Actions

    /// Moves the selected tab, with its running terminals, into a new window. A window's
    /// only tab stays where it is.
    pub fn move_tab_to_new_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() < 2 {
            return;
        }
        let index = self.selected;
        let tab = self.tabs.remove(index);
        self.selected = index.min(self.tabs.len() - 1);
        self.prune_groups();
        self.select(self.selected, window, cx);
        self.tabs_changed(cx);
        let bounds = window.bounds();
        let bounds = Bounds::new(bounds.origin + point(px(24.), px(24.)), bounds.size);
        cx.spawn(async move |_, cx| {
            cx.update(|cx| {
                let opened = crate::session::open_window(Some(bounds), cx, move |window, cx| {
                    cx.new(|cx| {
                        let mut workspace = Workspace::empty(cx);
                        workspace.adopt_tab(tab, window, cx);
                        workspace.start_git_polling(cx);
                        workspace.install_window_hooks(window, cx);
                        workspace
                    })
                });
                if opened.is_ok() {
                    cx.activate(true);
                }
            });
        })
        .detach();
    }

    /// Takes over a tab another window owned, as the only tab.
    fn adopt_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        let Tab { view, splits, color, title_override, .. } = tab;
        let splits = Splits::adopt(splits, window, cx);
        let id = self.next_tab_id();
        self.tabs.push(Tab { id, view, splits, color, title_override, group: None });
        self.select(0, window, cx);
        self.tabs_changed(cx);
    }

    pub fn new_tab_from_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let cwd = self.selected_view().read(cx).cwd();
        self.add_tab(cwd, window, cx);
    }

    /// Selects the tab `by` places away, wrapping around.
    pub fn step_tab(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len() as isize;
        let next = (self.selected as isize + by).rem_euclid(count);
        self.select(next as usize, window, cx);
    }

    /// Moves the selected tab `offset` places, wrapping around.
    pub fn move_selected_tab(&mut self, offset: isize, cx: &mut Context<Self>) {
        let count = self.tabs.len() as isize;
        if count < 2 {
            return;
        }
        let from = self.selected;
        let to = (from as isize + offset).rem_euclid(count) as usize;
        if from == to {
            return;
        }
        self.reorder(cx, |tabs| {
            let tab = tabs.remove(from);
            tabs.insert(to, tab);
        });
    }

    pub fn set_hovered(&mut self, key: RowKey, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            self.chrome.hovered = Some(key);
        } else if self.chrome.hovered == Some(key) {
            self.chrome.hovered = None;
        } else {
            return;
        }
        cx.notify();
    }

    pub fn select_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tab_index(id) {
            self.select(index, window, cx);
        }
    }

    /// Gives the keyboard back to the selected tab's terminal.
    pub fn focus_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focus = self.selected_view().read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    }

    // MARK: Group rename

    pub fn is_renaming(&self, group: Uuid) -> bool {
        self.chrome.renaming.as_ref().is_some_and(|r| r.group == group)
    }

    /// Edits the group's name in place.
    pub fn begin_group_rename(&mut self, group: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.chrome.groups.get(group).map(|info| info.name.clone()) else { return };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Group name").default_value(name));
        let subscription = cx.subscribe_in(&input, window, move |this, _, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                this.finish_group_rename(group, true, window, cx);
            }
        });
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        self.chrome.renaming = Some(GroupRename { group, input, _subscriptions: vec![subscription] });
        cx.notify();
    }

    /// Ends an in-place rename. Blank names change nothing.
    pub fn finish_group_rename(&mut self, group: Uuid, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_renaming(group) {
            return;
        }
        let Some(rename) = self.chrome.renaming.take() else { return };
        if commit {
            let name = rename.input.read(cx).value().to_string();
            self.chrome.groups.rename(group, &name);
        }
        self.focus_selected(window, cx);
        self.tabs_changed(cx);
    }
}

/// Opens a scripted set of tabs and groups when `ANGELLY_SELFTEST_TABS` is set. The value
/// is a comma separated list of options: `bar` hides the sidebar, `rename` starts renaming
/// a group, `collapse` collapses a group.
/// Test hook behind `ANGELLY_SELFTEST_WINDOWS`. `open` opens a second window, then quits
/// through the menu command, so the next launch shows what the session restored. `move`
/// moves a new tab into a window of its own.
fn selftest_windows(window: &mut Window, cx: &mut Context<Workspace>) {
    let Ok(mode) = std::env::var("ANGELLY_SELFTEST_WINDOWS") else { return };
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(Duration::from_millis(1200)).await;
        match mode.as_str() {
            "open" => {
                cx.update(|_, cx| drop(crate::app_host::open_window(&Default::default(), cx))).ok();
                cx.background_executor().timer(Duration::from_millis(2500)).await;
                cx.update(|_, cx| crate::platform::command::perform(crate::platform::command::Command::Quit, cx)).ok();
            }
            "move" => {
                this.update_in(cx, |this, window, cx| {
                    this.new_tab_from_selected(window, cx);
                    this.move_tab_to_new_window(window, cx);
                })
                .ok();
            }
            _ => {}
        }
    })
    .detach();
}

pub fn selftest(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    selftest_windows(window, cx);
    let Some(options) = std::env::var("ANGELLY_SELFTEST_TABS").ok() else { return };
    let options: Vec<&str> = options.split(',').collect();
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let repo = std::env::var_os("ANGELLY_SELFTEST_REPO").map(PathBuf::from).unwrap_or_else(|| home.clone());
    let directories = [repo.clone(), repo.join("rust"), PathBuf::from("/tmp"), home, repo.join("work"), PathBuf::from("/usr")];
    for directory in directories {
        workspace.add_tab(Some(directory), window, cx);
    }
    let names = [
        "angelly (main)",
        "build output with a rather long title that cannot fit",
        "ssh prod",
        "notes",
        "tests",
        "logs",
        "scratch",
    ];
    let colors = [
        TerminalTabColor::Blue,
        TerminalTabColor::None,
        TerminalTabColor::Orange,
        TerminalTabColor::Teal,
        TerminalTabColor::None,
        TerminalTabColor::Pink,
        TerminalTabColor::None,
    ];
    for (index, tab) in workspace.tabs.iter_mut().enumerate() {
        tab.title_override = names.get(index).map(|n| n.to_string());
        tab.color = colors[index % colors.len()];
    }
    let work = workspace.chrome.groups.create();
    let docs = workspace.chrome.groups.create();
    workspace.chrome.groups.rename(work, "Work");
    workspace.chrome.groups.rename(docs, "Docs");
    for index in [2, 3] {
        workspace.tabs[index].group = Some(work);
    }
    for index in [5, 6] {
        workspace.tabs[index].group = Some(docs);
    }
    if options.contains(&"collapse") {
        workspace.chrome.groups.update(docs, |info| info.is_collapsed = true);
    }
    if options.contains(&"bar") {
        workspace.sidebar_visible = false;
    }
    if options.contains(&"rename") {
        workspace.begin_group_rename(work, window, cx);
    }
    workspace.select(1, window, cx);
    if options.contains(&"theme-check") {
        selftest_theme(window, cx);
    }
    if options.contains(&"menu") {
        selftest_context_menu(window, cx);
    }
    if options.contains(&"hover") {
        cx.spawn_in(window, async move |_, cx| {
            for (x, y) in [(100., 48.)] {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                cx.update(|window, cx| {
                    window.dispatch_event(PlatformInput::MouseMove(MouseMoveEvent {
                        position: point(px(x), px(y)),
                        pressed_button: None,
                        modifiers: Modifiers::default(),
                    }), cx);
                }).ok();
            }
        }).detach();
    }
    if options.contains(&"hover-check") {
        crate::tab_popover::selftest(window, cx);
    }
}

/// `ANGELLY_SELFTEST_TABS=theme-check` checks unvisited tabs, then revisits them after another reload.
fn selftest_theme(window: &mut Window, cx: &mut Context<Workspace>) {
    cx.spawn_in(window, async move |this, cx| {
        use angelly_core::config::{ColorScheme, Config, Env};

        cx.background_executor().timer(Duration::from_millis(250)).await;
        for config in [
            "background = #f0e0d0\nforeground = #102030\npalette = 1=#a12345\ncursor-color = #123456\nfont-size = 15",
            "background = #102030\nforeground = #f0e0d0\npalette = 1=#b23456\ncursor-color = #654321\nfont-size = 16",
        ] {
            cx.update(|_, cx| {
                cx.set_global(Theme::from_config(Config::from_str(config, &Env::from_process(), ColorScheme::Dark)));
                crate::panels::ui::sync_component_theme(cx);
                cx.refresh_windows();
            }).unwrap();
            cx.background_executor().timer(Duration::from_millis(100)).await;
            let count = this.read_with(cx, |workspace, _| workspace.tabs.len()).unwrap();
            for index in 0..count {
                this.update_in(cx, |workspace, window, cx| {
                    for pane in workspace.tabs[index].splits.panes() {
                        pane.update(cx, |pane, cx| {
                            pane.session.term().update_frame(&mut pane.frame);
                            let theme = cx.global::<Theme>();
                            assert_eq!(pane.frame.colors.background, theme.colors.background, "tab {index} background before selection");
                            assert_eq!(pane.frame.colors.foreground, theme.colors.foreground, "tab {index} foreground before selection");
                            assert_eq!(pane.frame.colors.palette, theme.colors.palette, "tab {index} palette before selection");
                            assert_eq!(pane.frame.colors.cursor, theme.colors.cursor, "tab {index} cursor before selection");
                            assert_eq!(pane.font_size, theme.font_size, "tab {index} font size before selection");
                        });
                    }
                    workspace.select(index, window, cx);
                }).unwrap();
                cx.background_executor().timer(Duration::from_millis(100)).await;
                this.read_with(cx, |workspace, cx| {
                    let theme = cx.global::<Theme>();
                    for pane in workspace.tabs[index].splits.panes() {
                        assert_eq!(pane.read(cx).frame.colors.background, theme.colors.background, "tab {index} rendered background");
                    }
                }).unwrap();
            }
        }
        eprintln!("[selftest] theme-check passed: unvisited and previously rendered tabs follow theme reloads");
        cx.update(|_, cx| cx.quit()).unwrap();
    }).detach();
}

/// `ANGELLY_SELFTEST_TABS=sidebar,menu` previews the tab menu and its color submenu.
fn selftest_context_menu(window: &mut Window, cx: &mut Context<Workspace>) {
    cx.spawn_in(window, async move |_, cx| {
        cx.background_executor().timer(Duration::from_millis(500)).await;
        cx.update(|window, cx| {
            let component = gpui_kit::component::Theme::global(cx);
            assert!(component.popover.a < 1.0, "workspace startup reset the menu material");
            assert_eq!(component.mode.is_dark(), cx.global::<Theme>().dark);
            eprintln!("[selftest] context menu material {:?}", component.popover);
            window.dispatch_event(PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Right,
                position: point(px(80.), px(80.)),
                ..Default::default()
            }), cx);
        }).ok();
        #[cfg(not(target_os = "macos"))]
        for key in ["down", "down", "right"] {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            cx.update(|window, cx| {
                window.dispatch_event(PlatformInput::KeyDown(KeyDownEvent {
                    keystroke: Keystroke::parse(key).unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                }), cx);
            }).ok();
        }
    }).detach();
}
