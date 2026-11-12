//! The "New Worktree" popover.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use angelly_core::git::{default_folder_name, GitService, GitWorktreeSnapshot};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::ui::{self, text, Look, Variant};
use crate::prefs::Prefs;

pub enum WorktreeEvent {
    /// A worktree was created, or chosen, and should open in a new tab.
    Open(PathBuf),
    Dismissed,
}

impl EventEmitter<WorktreeEvent> for WorktreeCreate {}

/// `<root>/<repository folder>/<folder name>`.
pub fn destination(root: &Path, repository: &Path, folder: &str) -> PathBuf {
    let name = repository.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    root.join(name).join(folder)
}

/// The folder name to use: the typed override, else the branch flattened.
pub fn folder_for(branch: &str, override_name: &str) -> String {
    let typed = override_name.trim();
    if typed.is_empty() {
        default_folder_name(branch)
    } else {
        typed.to_string()
    }
}

/// Branches that can still be checked out: those no worktree has.
pub fn available_branches(snapshot: &GitWorktreeSnapshot) -> Vec<String> {
    snapshot
        .branches
        .iter()
        .filter(|b| !snapshot.worktrees.iter().any(|w| w.branch.as_deref() == Some(b.as_str())))
        .cloned()
        .collect()
}

pub struct WorktreeCreate {
    git: Arc<GitService>,
    directory: Option<PathBuf>,
    snapshot: Option<GitWorktreeSnapshot>,
    busy: bool,
    error: Option<String>,
    uses_existing: bool,
    existing: String,
    branch_input: Entity<InputState>,
    folder_input: Entity<InputState>,
    show_options: bool,
    picker_open: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl WorktreeCreate {
    pub fn new(git: Arc<GitService>, directory: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let branch_input = ui::new_input(window, cx, "feature/my-change", "");
        let folder_input = ui::new_input(window, cx, "", "");
        let subscriptions = vec![
            cx.subscribe_in(&branch_input, window, |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.create(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
            cx.subscribe(&folder_input, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }),
        ];
        branch_input.update(cx, |state, cx| state.focus(window, cx));
        let mut this = Self {
            git,
            directory,
            snapshot: None,
            busy: false,
            error: None,
            uses_existing: false,
            existing: String::new(),
            branch_input,
            folder_input,
            show_options: false,
            picker_open: false,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.load(cx);
        this
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(directory) = self.directory.clone() else { return };
        self.busy = true;
        let git = self.git.clone();
        cx.spawn(async move |this, cx| {
            let snapshot = cx.background_spawn(async move { git.load(&directory).ok() }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if let Some(snapshot) = &snapshot {
                    this.existing = available_branches(snapshot).into_iter().next().unwrap_or_default();
                }
                this.snapshot = snapshot;
                cx.notify();
            });
        })
        .detach();
    }

    fn selected_branch(&self, cx: &App) -> String {
        if self.uses_existing {
            self.existing.clone()
        } else {
            self.branch_input.read(cx).value().trim().to_string()
        }
    }

    fn selected_folder(&self, cx: &App) -> String {
        folder_for(&self.selected_branch(cx), &self.folder_input.read(cx).value())
    }

    fn can_create(&self, cx: &App) -> bool {
        self.snapshot.is_some() && !self.selected_branch(cx).is_empty() && !self.busy
    }

    fn can_dismiss(&self) -> bool {
        !self.busy || self.snapshot.is_none()
    }

    pub(super) fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.can_dismiss() {
            cx.emit(WorktreeEvent::Dismissed);
        }
    }

    fn create(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if !self.can_create(cx) {
            return;
        }
        let (Some(snapshot), branch, folder) = (self.snapshot.clone(), self.selected_branch(cx), self.selected_folder(cx)) else {
            return;
        };
        let source = self.directory.clone().unwrap_or_else(|| snapshot.directory.clone());
        let root = Prefs::read(cx).worktree_root();
        let new_branch = !self.uses_existing;
        let git = self.git.clone();
        self.busy = true;
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_spawn(async move { git.create(&source, &root, &folder, &branch, new_branch) }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(path) => cx.emit(WorktreeEvent::Open(path)),
                    Err(error) => {
                        this.error = Some(error.message);
                        // Folder collisions (`a/b` against `a-b`) are fixed in Options.
                        this.show_options = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn branch_field(&mut self, look: &Look, cx: &mut Context<Self>) -> Div {
        let mut column = div().v_flex().gap(px(12.));
        let snapshot = self.snapshot.as_ref();
        if self.uses_existing {
            let available = snapshot.map(available_branches).unwrap_or_default();
            let field = if available.is_empty() {
                div().text_size(px(text::CALLOUT)).text_color(look.secondary).child("Every local branch is already checked out.")
            } else {
                let current = self.existing.clone();
                let mut holder = div().relative().child(
                    ui::popup_face("worktree-branch-picker", current.clone(), look)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.picker_open = !this.picker_open;
                            cx.notify();
                        })),
                );
                if self.picker_open {
                    let items = available.iter().enumerate().map(|(index, branch)| {
                        let value = branch.clone();
                        ui::menu_item(("worktree-branch", index), branch.clone(), *branch == current, look).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.existing = value.clone();
                                this.picker_open = false;
                                cx.notify();
                            }),
                        )
                    });
                    holder = holder.child(deferred(
                        anchored()
                            .anchor(Anchor::TopLeft)
                            .offset(point(px(0.), px(24.)))
                            .snap_to_window_with_margin(px(8.))
                            .child(ui::menu(look).max_h(px(220.)).overflow_hidden().children(items)),
                    ));
                }
                holder
            };
            column = column.child(labeled("Branch", look, field));
        } else {
            column = column.child(labeled("Branch name", look, ui::field(&self.branch_input)));
        }
        column.child(
            ui::link(
                "worktree-toggle-mode",
                if self.uses_existing { "Create a new branch instead" } else { "Use an existing branch…" },
                look,
                text::CAPTION,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.uses_existing = !this.uses_existing;
                this.picker_open = false;
                this.error = None;
                cx.notify();
            })),
        )
    }
}

fn labeled(title: &str, look: &Look, content: impl IntoElement) -> Div {
    div()
        .v_flex()
        .gap(px(4.))
        .child(div().text_size(px(text::SUBHEADLINE)).text_color(look.secondary).child(title.to_string()))
        .child(content)
}

impl Render for WorktreeCreate {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let mut body = div()
            .v_flex()
            .gap(px(12.))
            .child(div().text_size(px(text::HEADLINE)).font_weight(FontWeight::BOLD).child("New Worktree"));

        if let Some(snapshot) = self.snapshot.clone() {
            let root = Prefs::read(cx).worktree_root();
            let target = destination(&root, &snapshot.directory, &self.selected_folder(cx)).display().to_string();
            body = body
                .child(self.branch_field(&look, cx))
                .child(
                    div()
                        .text_size(px(text::CAPTION))
                        .text_color(look.secondary)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(middle_truncate(&target, 52)),
                )
                .child(
                    div()
                        .id("worktree-options")
                        .h_flex()
                        .gap(px(4.))
                        .items_center()
                        .text_size(px(text::BODY))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_options = !this.show_options;
                            cx.notify();
                        }))
                        .child(div().w(px(10.)).child(ui::disclosure(self.show_options, &look)))
                        .child("Options"),
                );
            if self.show_options {
                body = body.child(labeled("Folder name", &look, ui::field(&self.folder_input)));
            }
        } else if !self.busy {
            body = body.child(
                div()
                    .text_size(px(text::CALLOUT))
                    .text_color(look.secondary)
                    .child("Open a terminal in a Git repository to create a worktree."),
            );
        }

        if let Some(error) = &self.error {
            body = body.child(div().text_size(px(text::CAPTION)).text_color(look.red).child(error.clone()));
        }

        let can_create = self.can_create(cx);
        let can_dismiss = self.can_dismiss();
        body = body.child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .child(div().when(self.busy, |d| d.child(ui::spinner("worktree-busy", 14., look.primary, 1.0))))
                .child(
                    div()
                        .h_flex()
                        .gap(px(8.))
                        .child(
                            ui::button("worktree-cancel", "Cancel", &look, Variant::Regular, false, can_dismiss)
                                .when(can_dismiss, |b| b.on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)))),
                        )
                        .child(
                            ui::button("worktree-create", "Create and Open", &look, Variant::Prominent, false, can_create).when(
                                can_create,
                                |b| b.on_click(cx.listener(|this, _, window, cx| this.create(window, cx))),
                            ),
                        ),
                ),
        );

        ui::card(&look, 10.)
            .id("worktree-create")
            .key_context("WorktreeCreate")
            .track_focus(&self.focus)
            .occlude()
            .w(px(340.))
            .p(px(16.))
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.dismiss(cx);
                    cx.stop_propagation();
                }
            }))
            .child(body)
    }
}

/// Shortens the middle of a long path with an ellipsis.
pub fn middle_truncate(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars || max_chars < 3 {
        return text.to_string();
    }
    let keep = max_chars - 1;
    let tail = keep / 2;
    let head = keep - tail;
    format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;
    use angelly_core::git::GitWorktree;

    fn worktree(branch: Option<&str>) -> GitWorktree {
        GitWorktree {
            directory: PathBuf::from("/r"),
            branch: branch.map(str::to_string),
            is_main: false,
            is_bare: false,
            is_locked: false,
            is_prunable: false,
        }
    }

    #[test]
    fn destination_nests_under_the_repository_name() {
        let path = destination(Path::new("/home/me/worktrees"), Path::new("/src/angelly"), "feature-x");
        assert_eq!(path, PathBuf::from("/home/me/worktrees/angelly/feature-x"));
    }

    #[test]
    fn folder_name_defaults_to_the_flattened_branch() {
        assert_eq!(folder_for("feature/my-change", ""), "feature-my-change");
        assert_eq!(folder_for("feature/my-change", "  "), "feature-my-change");
        assert_eq!(folder_for("feature/my-change", " custom "), "custom");
    }

    #[test]
    fn checked_out_branches_are_not_offered() {
        let snapshot = GitWorktreeSnapshot {
            directory: PathBuf::from("/r"),
            worktrees: vec![worktree(Some("main")), worktree(None), worktree(Some("wip"))],
            branches: vec!["main".into(), "wip".into(), "idea".into()],
        };
        assert_eq!(available_branches(&snapshot), ["idea"]);
    }

    #[test]
    fn long_paths_truncate_in_the_middle() {
        assert_eq!(middle_truncate("/short", 20), "/short");
        let long = "/Users/me/worktrees/angelly/feature-my-change";
        let cut = middle_truncate(long, 21);
        assert_eq!(cut.chars().count(), 21);
        assert!(cut.starts_with("/Users/me/") && cut.ends_with("-change") && cut.contains('…'));
    }
}
