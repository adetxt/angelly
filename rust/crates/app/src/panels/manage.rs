//! The "Manage Worktrees" sheet.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use angelly_core::git::{GitService, GitWorktree, GitWorktreeSnapshot};
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::confirm::{Confirm, ConfirmView, Dismissed};
use super::ui::{self, text, Look, Variant};
use super::worktree::WorktreeEvent;
use crate::prefs::Prefs;

impl EventEmitter<WorktreeEvent> for WorktreeManage {}

/// Directories of terminals that already live in a worktree.
pub fn terminal_uses_worktree(terminal_dirs: &[PathBuf], worktree: &Path) -> bool {
    terminal_dirs.iter().any(|dir| {
        let resolved = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        resolved.starts_with(worktree)
    })
}

/// What a worktree row's title shows.
pub fn worktree_title(worktree: &GitWorktree) -> String {
    match (&worktree.branch, worktree.is_bare) {
        (Some(branch), _) => branch.clone(),
        (None, true) => "Bare repository".to_string(),
        (None, false) => "Detached HEAD".to_string(),
    }
}

pub struct WorktreeManage {
    git: Arc<GitService>,
    directory: Option<PathBuf>,
    terminal_dirs: Vec<PathBuf>,
    repositories: Vec<GitWorktreeSnapshot>,
    selected: Option<PathBuf>,
    busy: bool,
    error: Option<String>,
    picker_open: bool,
    confirm: Option<Entity<ConfirmView>>,
    focus: FocusHandle,
    _subscription: Option<Subscription>,
}

impl WorktreeManage {
    pub fn new(
        git: Arc<GitService>,
        directory: Option<PathBuf>,
        terminal_dirs: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut this = Self {
            git,
            directory,
            terminal_dirs,
            repositories: Vec::new(),
            selected: None,
            busy: false,
            error: None,
            picker_open: false,
            confirm: None,
            focus,
            _subscription: None,
        };
        this.refresh(cx);
        this
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        let mut directories: Vec<PathBuf> = self.repositories.iter().map(|r| r.directory.clone()).collect();
        directories.extend(self.directory.clone());
        directories.extend(self.terminal_dirs.iter().filter(|d| d.is_absolute()).cloned());
        let root = Prefs::read(cx).worktree_root();
        let git = self.git.clone();
        cx.spawn(async move |this, cx| {
            let repositories = cx.background_spawn(async move { git.load_repositories(&directories, &root) }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                if let Some(selected) = &this.selected {
                    if !repositories.iter().any(|r| &r.directory == selected) {
                        this.selected = None;
                    }
                }
                this.repositories = repositories;
                this.error = None;
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_repository(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a Git repository.".into()),
        });
        let git = self.git.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(directory) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |this, cx| {
                this.busy = true;
                cx.notify();
            });
            let result = cx.background_spawn(async move { git.load(&directory) }).await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(snapshot) => {
                        this.repositories.retain(|r| r.directory != snapshot.directory);
                        this.selected = Some(snapshot.directory.clone());
                        this.repositories.push(snapshot);
                        this.repositories.sort_by(|a, b| a.directory.cmp(&b.directory));
                        this.error = None;
                    }
                    Err(error) => this.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn prune(&mut self, repository: PathBuf, cx: &mut Context<Self>) {
        self.run_git(repository, cx, |git, repository| git.prune(repository));
    }

    fn remove(&mut self, repository: PathBuf, worktree: PathBuf, cx: &mut Context<Self>) {
        if terminal_uses_worktree(&self.terminal_dirs, &worktree) {
            self.error = Some(
                "A terminal is still using this worktree. Close it or change its directory before removing the worktree."
                    .to_string(),
            );
            cx.notify();
            return;
        }
        self.run_git(repository, cx, move |git, repository| git.remove(repository, &worktree));
    }

    /// Runs a Git operation on a repository, then reloads that repository.
    fn run_git(
        &mut self,
        repository: PathBuf,
        cx: &mut Context<Self>,
        operation: impl FnOnce(&GitService, &Path) -> Result<(), angelly_core::git::GitError> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let git = self.git.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    operation(&git, &repository)?;
                    git.load(&repository)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(snapshot) => {
                        if let Some(slot) = this.repositories.iter_mut().find(|r| r.directory == snapshot.directory) {
                            *slot = snapshot;
                        }
                    }
                    Err(error) => this.error = Some(error.message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_directory(&mut self, directory: PathBuf, cx: &mut Context<Self>) {
        if directory.is_dir() {
            cx.emit(WorktreeEvent::Open(directory));
        } else {
            self.error = Some("This worktree directory is unavailable. Refresh the list and try again.".to_string());
            cx.notify();
        }
    }

    fn ask_remove(&mut self, repository: PathBuf, worktree: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        let request = Confirm::remove_worktree(&worktree.display().to_string(), move |_, cx| {
            let _ = this.update(cx, |this, cx| this.remove(repository, worktree, cx));
        });
        let view = cx.new(|cx| ConfirmView::new(request, window, cx));
        self._subscription = Some(cx.subscribe(&view, |this, _, _: &Dismissed, cx| {
            this.confirm = None;
            cx.notify();
        }));
        self.confirm = Some(view);
        cx.notify();
    }

    fn worktree_row(&self, repository: &Path, worktree: &GitWorktree, index: usize, look: &Look, cx: &mut Context<Self>) -> Div {
        let mut details = div().v_flex().gap(px(4.)).min_w_0().flex_1().child(
            div().text_size(px(text::BODY)).font_weight(FontWeight::MEDIUM).child(worktree_title(worktree)),
        );
        details = details.child(
            div().text_size(px(text::CAPTION)).text_color(look.secondary).child(worktree.directory.display().to_string()),
        );
        for (on, label) in [
            (worktree.is_main, "Main checkout"),
            (worktree.is_locked, "Locked"),
            (worktree.is_prunable, "Missing directory"),
        ] {
            if on {
                details = details.child(div().text_size(px(text::CAPTION)).child(label));
            }
        }
        let can_open = !worktree.is_bare && !worktree.is_prunable;
        let can_remove = worktree.can_remove() && !self.busy;
        let (open_dir, remove_dir, repo) = (worktree.directory.clone(), worktree.directory.clone(), repository.to_path_buf());
        div()
            .v_flex()
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(px(12.))
                    .py(px(10.))
                    .child(details)
                    .child(ui::button(("open-tab", index), "Open Tab", look, Variant::Regular, false, can_open && !self.busy).when(
                        can_open && !self.busy,
                        |b| b.on_click(cx.listener(move |this, _, _, cx| this.open_directory(open_dir.clone(), cx))),
                    ))
                    .child(
                        ui::button(("remove", index), "Remove…", look, Variant::Destructive, false, can_remove).when(
                            can_remove,
                            |b| {
                                b.on_click(cx.listener(move |this, _, window, cx| {
                                    this.ask_remove(repo.clone(), remove_dir.clone(), window, cx)
                                }))
                            },
                        ),
                    ),
            )
            .child(ui::divider(look))
    }

    fn repository_section(&self, snapshot: &GitWorktreeSnapshot, number: usize, look: &Look, cx: &mut Context<Self>) -> Div {
        let name = snapshot.directory.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let has_missing = snapshot.worktrees.iter().any(|w| w.is_prunable);
        let repo = snapshot.directory.clone();
        let rows: Vec<Div> = snapshot
            .worktrees
            .iter()
            .enumerate()
            .map(|(index, w)| self.worktree_row(&snapshot.directory, w, number * 1000 + index, look, cx))
            .collect();
        div()
            .v_flex()
            .gap(px(4.))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(text::HEADLINE)).font_weight(FontWeight::BOLD).child(name))
                    .when(has_missing, |d| {
                        d.child(
                            ui::button(("prune", number), "Prune Missing", look, Variant::Regular, true, !self.busy).on_click(
                                cx.listener(move |this, _, _, cx| this.prune(repo.clone(), cx)),
                            ),
                        )
                    }),
            )
            .child(div().text_size(px(text::CALLOUT)).text_color(look.secondary).child(snapshot.directory.display().to_string()))
            .child(div().v_flex().children(rows))
    }
}

impl Render for WorktreeManage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let picker_label = match &self.selected {
            Some(selected) => selected.display().to_string(),
            None => "All repositories".to_string(),
        };

        let mut picker = div().relative().child(
            ui::popup_face("manage-repo-picker", picker_label, &look).on_click(cx.listener(|this, _, _, cx| {
                this.picker_open = !this.picker_open;
                cx.notify();
            })),
        );
        if self.picker_open {
            let mut items = vec![ui::menu_item("manage-all", "All repositories", self.selected.is_none(), &look)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.selected = None;
                    this.picker_open = false;
                    cx.notify();
                }))];
            for (index, repository) in self.repositories.iter().enumerate() {
                let directory = repository.directory.clone();
                let selected = self.selected.as_ref() == Some(&directory);
                items.push(
                    ui::menu_item(("manage-repo", index), directory.display().to_string(), selected, &look).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.selected = Some(directory.clone());
                            this.picker_open = false;
                            cx.notify();
                        }),
                    ),
                );
            }
            picker = picker.child(deferred(
                anchored()
                    .anchor(Anchor::TopLeft)
                    .offset(point(px(0.), px(24.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(ui::menu(&look).children(items)),
            ));
        }

        let visible: Vec<GitWorktreeSnapshot> = self
            .repositories
            .iter()
            .filter(|r| self.selected.as_ref().is_none_or(|s| s == &r.directory))
            .cloned()
            .collect();

        let list = if self.repositories.is_empty() {
            div()
                .h(px(120.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(text::BODY))
                .child("No repositories found in open terminals or the worktree folder. Choose a repository to manage its worktrees.")
                .into_any_element()
        } else {
            let sections: Vec<Div> =
                visible.iter().enumerate().map(|(i, snapshot)| self.repository_section(snapshot, i, &look, cx)).collect();
            div()
                .id("manage-list")
                .h(px(260.))
                .overflow_y_scroll()
                .child(div().v_flex().gap(px(16.)).children(sections))
                .into_any_element()
        };

        let mut content = div()
            .v_flex()
            .gap(px(16.))
            .p(px(20.))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(text::TITLE2)).child("Manage Worktrees"))
                    .child(
                        ui::button("manage-refresh", "Refresh", &look, Variant::Regular, false, !self.busy)
                            .when(!self.busy, |b| b.on_click(cx.listener(|this, _, _, cx| this.refresh(cx)))),
                    ),
            )
            .child(
                div().h_flex().items_center().gap(px(8.)).child(
                    div().text_size(px(text::BODY)).child("Repository"),
                ).child(picker).child(
                    ui::button("manage-choose", "Choose Repository…", &look, Variant::Regular, false, !self.busy)
                        .when(!self.busy, |b| b.on_click(cx.listener(|this, _, _, cx| this.choose_repository(cx)))),
                ),
            )
            .child(list);

        if let Some(error) = &self.error {
            content = content.child(
                div()
                    .id("manage-error")
                    .max_h(px(90.))
                    .overflow_y_scroll()
                    .text_size(px(text::BODY))
                    .text_color(look.red)
                    .child(error.clone()),
            );
        }
        content = content.child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .h_flex()
                        .gap(px(6.))
                        .items_center()
                        .text_size(px(text::CALLOUT))
                        .when(self.busy, |d| {
                            d.child(ui::spinner("manage-busy", 12., look.primary, 1.0)).child("Running Git…")
                        }),
                )
                .child(
                    ui::button("manage-done", "Done", &look, Variant::Prominent, false, !self.busy)
                        .when(!self.busy, |b| b.on_click(cx.listener(|_, _, _, cx| cx.emit(WorktreeEvent::Dismissed)))),
                ),
        );

        ui::card(&look, 12.)
            .occlude()
            .w(px(680.))
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .id("worktree-manage")
            .key_context("WorktreeManage")
            .track_focus(&self.focus)
            .relative()
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && !this.busy && this.confirm.is_none() {
                    cx.emit(WorktreeEvent::Dismissed);
                }
            }))
            .child(content)
            .children(self.confirm.clone().map(|confirm| {
                div().absolute().inset_0().flex().items_start().justify_center().pt(px(40.)).child(confirm)
            }))
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    fn worktree(branch: Option<&str>, bare: bool) -> GitWorktree {
        GitWorktree {
            directory: PathBuf::from("/r"),
            branch: branch.map(str::to_string),
            is_main: false,
            is_bare: bare,
            is_locked: false,
            is_prunable: false,
        }
    }

    #[test]
    fn row_titles_describe_the_checkout() {
        assert_eq!(worktree_title(&worktree(Some("main"), false)), "main");
        assert_eq!(worktree_title(&worktree(None, true)), "Bare repository");
        assert_eq!(worktree_title(&worktree(None, false)), "Detached HEAD");
    }

    #[test]
    fn terminals_inside_a_worktree_block_its_removal() {
        let dirs = vec![PathBuf::from("/tmp/angelly-test/wt/sub"), PathBuf::from("/elsewhere")];
        assert!(terminal_uses_worktree(&dirs, Path::new("/tmp/angelly-test/wt")));
        assert!(!terminal_uses_worktree(&dirs, Path::new("/tmp/angelly-test/other")));
        // A sibling that only shares a name prefix is not inside the worktree.
        assert!(!terminal_uses_worktree(&[PathBuf::from("/tmp/angelly-test/wt-2")], Path::new("/tmp/angelly-test/wt")));
    }
}
