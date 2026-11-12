//! The app's preferences file, owned by one entity.
//!
//! Every window and panel reads and writes through [`Prefs`], so there is a single copy
//! in memory and a single writer on disk. Observers are notified of each change, which
//! keeps the windows of the app in step (the sidebar state is shared between them).

use std::path::{Path, PathBuf};

use angelly_core::config::Env;
use angelly_core::persist::{
    default_worktree_root, HorizontalNewTabButtonPlacement, Preferences, PREFERENCES_FILE_NAME,
};
use gpui_kit::*;

pub struct Prefs {
    /// `None` for self tests and when the file cannot be read, so it is never overwritten.
    path: Option<PathBuf>,
    prefs: Preferences,
    default_root: PathBuf,
}

struct PrefsHandle(Entity<Prefs>);

impl Global for PrefsHandle {}

impl Prefs {
    /// Loads the preferences and registers the owner. Call once at startup.
    pub fn init(cx: &mut App) {
        let env = Env::from_process();
        let path = crate::session::data_directory().map(|dir| dir.join(PREFERENCES_FILE_NAME));
        let default_root = default_worktree_root(&env).unwrap_or_else(|| PathBuf::from("worktrees"));
        let owner = cx.new(|_| Prefs::open(path.as_deref(), default_root));
        cx.set_global(PrefsHandle(owner));
    }

    fn open(path: Option<&Path>, default_root: PathBuf) -> Self {
        let (path, prefs) = match path.map(|path| (path, Preferences::load(path))) {
            Some((path, Ok(prefs))) => (Some(path.to_path_buf()), prefs),
            Some((_, Err(_))) | None => (None, Preferences::default()),
        };
        Self { path, prefs, default_root }
    }

    /// The owner, for observing changes.
    pub fn entity(cx: &App) -> Entity<Prefs> {
        cx.global::<PrefsHandle>().0.clone()
    }

    pub fn read(cx: &App) -> &Prefs {
        cx.global::<PrefsHandle>().0.read(cx)
    }

    /// Applies `change`, saves the file and tells the observers.
    pub fn update(cx: &mut App, change: impl FnOnce(&mut Preferences)) {
        let owner = Self::entity(cx);
        owner.update(cx, |this, cx| {
            change(&mut this.prefs);
            if let Some(path) = &this.path {
                let _ = this.prefs.save(path);
            }
            cx.notify();
        });
    }

    pub fn preferences(&self) -> &Preferences {
        &self.prefs
    }

    pub fn shows_quota(&self) -> bool {
        self.prefs.shows_ai_quota_tracker()
    }

    pub fn worktree_root(&self) -> PathBuf {
        self.prefs.worktree_root(&self.default_root)
    }

    pub fn worktree_root_is_default(&self) -> bool {
        self.worktree_root() == self.default_root
    }

    pub fn new_tab_placement(&self) -> HorizontalNewTabButtonPlacement {
        self.prefs.horizontal_new_tab_button_placement()
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn a_missing_file_gives_defaults_and_is_written_later() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        let prefs = Prefs::open(Some(&path), PathBuf::from("/w"));
        assert!(prefs.path.is_some());
        assert!(prefs.shows_quota());
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.json");
        std::fs::write(&path, "[1, 2]").unwrap();
        let prefs = Prefs::open(Some(&path), PathBuf::from("/w"));
        assert!(prefs.path.is_none());
    }

    #[test]
    fn self_tests_have_no_file() {
        assert!(Prefs::open(None, PathBuf::from("/w")).path.is_none());
    }
}
