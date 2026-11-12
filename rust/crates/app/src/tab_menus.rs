//! Context menus of tabs and tab groups, and the rename-tab prompt.

use angelly_core::persist::TerminalTabColor;
use gpui_kit::*;
use uuid::Uuid;

use crate::menu::Menu;
use crate::panels::Confirm;
use crate::tab_colors::display_color;
use crate::tabs::CloseScope;
use crate::theme::Theme;
use crate::workspace::{TabId, Workspace};

fn item(
    workspace: &WeakEntity<Workspace>,
    action: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> impl Fn(&mut Window, &mut App) {
    let workspace = workspace.clone();
    move |window, cx| {
        workspace.update(cx, |workspace, cx| action(workspace, window, cx)).ok();
    }
}

fn color_items(
    menu: Menu,
    workspace: &WeakEntity<Workspace>,
    apply: impl Fn(&mut Workspace, TerminalTabColor, &mut Context<Workspace>) + Clone + 'static,
    cx: &App,
) -> Menu {
    let dark = cx.global::<Theme>().dark;
    TerminalTabColor::ALL.into_iter().fold(menu, |menu, color| {
        let workspace = workspace.clone();
        let apply = apply.clone();
        menu.color_item(color.localized_name(), display_color(color, dark), move |_, cx| {
            workspace.update(cx, |workspace, cx| apply(workspace, color, cx)).ok();
        })
    })
}

/// The right-click menu of a tab.
pub fn tab_menu(id: TabId, workspace: &WeakEntity<Workspace>, cx: &App) -> Menu {
    let menu = Menu::new();
    let Some(entity) = workspace.upgrade() else { return menu };
    let (group, others) = {
        let ws = entity.read(cx);
        let group = ws.tabs.iter().find(|t| t.id == id).and_then(|t| t.group);
        let others: Vec<(Uuid, String)> = ws
            .rows(cx)
            .iter()
            .filter_map(|row| match row {
                angelly_core::persist::Row::Group { header, .. } => Some((header.id, header.name.clone())),
                _ => None,
            })
            .fold(Vec::new(), |mut seen, entry| {
                if !seen.iter().any(|(g, _)| *g == entry.0) && Some(entry.0) != group {
                    seen.push(entry);
                }
                seen
            });
        (group, others)
    };

    let menu = if let Some(angelly_core::git::GitPullRequestStatus::PullRequest { url, .. }) =
        entity.read(cx).chrome.git.status_for(id).map(|status| &status.pull_request)
    {
        let url = url.to_string();
        menu.item("Open Pull Request", move |_, cx| cx.open_url(&url)).separator()
    } else {
        menu
    };
    let menu = menu
        .item(
            "Rename Tab…",
            item(workspace, move |ws, window, cx| ws.prompt_rename_tab(id, window, cx)),
        )
        .submenu(
            "Tab Color",
            color_items(
                Menu::new(),
                workspace,
                move |ws, color, cx| ws.set_tab_color(id, color, cx),
                cx,
            ),
        )
        .submenu("Add to Group", {
            let menu = Menu::new().item(
                "New Group",
                item(workspace, move |ws, window, cx| ws.new_group_with(id, window, cx)),
            );
            let menu = if others.is_empty() { menu } else { menu.separator() };
            others.iter().fold(menu, |menu, (group, name)| {
                let group = *group;
                let workspace = workspace.clone();
                menu.item(name.clone(), move |_, cx| {
                    workspace.update(cx, |ws, cx| ws.add_to_group(id, group, cx)).ok();
                })
            })
        });
    let menu = if group.is_some() {
        menu.item(
            "Remove from Group",
            item(workspace, move |ws, _, cx| ws.leave_group(id, cx)),
        )
    } else {
        menu
    };
    menu.separator()
        .item(
            "Close Tab",
            item(workspace, move |ws, window, cx| {
                ws.close_scope(CloseScope::Tab(id), window, cx)
            }),
        )
        .item(
            "Close Other Tabs",
            item(workspace, move |ws, window, cx| {
                ws.close_scope(CloseScope::Others(id), window, cx)
            }),
        )
        .item(
            "Close Tabs to the Right",
            item(workspace, move |ws, window, cx| {
                ws.close_scope(CloseScope::Right(id), window, cx)
            }),
        )
}

/// The right-click menu of a group header.
pub fn group_menu(group: Uuid, collapsed: bool, workspace: &WeakEntity<Workspace>, cx: &App) -> Menu {
    Menu::new()
        .item(
            "Rename Group…",
            item(workspace, move |ws, window, cx| {
                ws.begin_group_rename(group, window, cx)
            }),
        )
        .submenu(
            "Group Color",
            color_items(
                Menu::new(),
                workspace,
                move |ws, color, cx| ws.set_group_color(group, color, cx),
                cx,
            ),
        )
        .separator()
        .item(
            if collapsed { "Expand Group" } else { "Collapse Group" },
            item(workspace, move |ws, _, cx| ws.toggle_collapsed(group, cx)),
        )
        .item("Ungroup", item(workspace, move |ws, _, cx| ws.ungroup(group, cx)))
        .item(
            "Close Group",
            item(workspace, move |ws, window, cx| {
                ws.close_scope(CloseScope::Group(group), window, cx)
            }),
        )
}

impl Workspace {
    /// "Change Tab Title": a prompt with the current title; blank restores the default.
    pub fn prompt_rename_tab(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        let current = tab.title_override.clone().unwrap_or_else(|| self.tab_title(tab, cx));
        let workspace = cx.entity().downgrade();
        let request = Confirm::prompt_title(&current, move |title, window, cx| {
            workspace
                .update(cx, |ws, cx| {
                    ws.set_title_override(id, &title, cx);
                    ws.focus_selected(window, cx);
                })
                .ok();
        });
        self.ask(request, window, cx);
    }
}
