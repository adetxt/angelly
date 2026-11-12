//! Documents opened through the Dock icon, Finder's "Open With" and `open -a`
//! (spec 16.7). Folders open a terminal in them; executables and shell scripts run
//! after the user confirms.

use std::path::{Path, PathBuf};

use angelly_core::config::Config;

use super::command::{self, Command};
use super::runtime;
use super::services::percent_decode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropBehavior {
    NewTab,
    NewWindow,
}

pub fn drop_behavior(config: &Config) -> DropBehavior {
    match config.get("macos-dock-drop-behavior") {
        Some("new-window") => DropBehavior::NewWindow,
        _ => DropBehavior::NewTab,
    }
}

/// File paths among the URLs the system hands over. Other schemes are ignored.
pub fn file_paths(urls: &[String]) -> Vec<PathBuf> {
    urls.iter()
        .filter_map(|url| url.strip_prefix("file://"))
        .map(|rest| rest.strip_prefix("localhost").unwrap_or(rest))
        .map(|rest| PathBuf::from(percent_decode(rest)))
        .collect()
}

/// Whether a path holds characters that act as keystrokes when typed into a shell.
/// Quoting cannot protect against them, and the confirmation dialog would hide them.
pub fn has_control_characters(path: &str) -> bool {
    path.chars().any(char::is_control)
}

/// Single-quotes `text` for a POSIX shell.
pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The keystrokes that run `path` and leave the shell when it finishes.
pub fn run_input(path: &Path) -> String {
    format!("{}; exit\n", shell_quote(&path.to_string_lossy()))
}

/// Entry point for `application:openURLs:`. Calls made before the app is ready are kept
/// and replayed by [`flush_pending`].
pub fn open_urls(urls: Vec<String>) {
    let handled = runtime::with_app(|cx| open_paths(file_paths(&urls), cx));
    if handled.is_none() {
        PENDING.with(|pending| pending.borrow_mut().extend(urls));
    }
}

thread_local! {
    static PENDING: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn flush_pending(cx: &mut gpui_kit::App) {
    let urls = PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    if !urls.is_empty() {
        open_paths(file_paths(&urls), cx);
    }
}

pub fn open_paths(paths: Vec<PathBuf>, cx: &mut gpui_kit::App) {
    let behavior = cx
        .try_global::<crate::theme::Theme>()
        .map(|theme| drop_behavior(&theme.config))
        .unwrap_or(DropBehavior::NewTab);
    let (folders, files): (Vec<_>, Vec<_>) = paths.into_iter().filter(|p| p.exists()).partition(|p| p.is_dir());
    if !folders.is_empty() {
        let tabs = behavior == DropBehavior::NewTab;
        command::perform(Command::OpenDirectories { paths: folders, tabs }, cx);
    }
    for file in files {
        run_file(file, behavior == DropBehavior::NewTab, cx);
    }
}

fn run_file(path: PathBuf, tab: bool, cx: &mut gpui_kit::App) {
    let text = path.to_string_lossy();
    if has_control_characters(&text) {
        alert::refuse(&text);
        return;
    }
    if alert::confirm_execute(&text) {
        command::perform(Command::RunFile { path, tab }, cx);
    }
}

#[cfg(target_os = "macos")]
mod alert {
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSAlertStyle};
    use objc2_foundation::{MainThreadMarker, NSString};

    pub fn refuse(path: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str("Cannot Execute File"));
        alert.setInformativeText(&NSString::from_str(
            "This file's path contains hidden control characters, which could make Angelly run a different command than the one shown.",
        ));
        alert.addButtonWithTitle(&NSString::from_str("OK"));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.runModal();
        let _ = path;
    }

    pub fn confirm_execute(path: &str) -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&format!("Allow Angelly to execute \"{path}\"?")));
        alert.addButtonWithTitle(&NSString::from_str("Allow"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.runModal() == NSAlertFirstButtonReturn
    }
}

#[cfg(not(target_os = "macos"))]
mod alert {
    pub fn refuse(_: &str) {}

    pub fn confirm_execute(_: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use angelly_core::config::{ColorScheme, Env, Platform};

    use super::*;

    #[test]
    fn urls_become_file_paths() {
        let urls = vec![
            "file:///Users/me/My%20Folder/".to_string(),
            "https://example.com".to_string(),
            "file://localhost/tmp/x".to_string(),
        ];
        assert_eq!(file_paths(&urls), vec![PathBuf::from("/Users/me/My Folder/"), PathBuf::from("/tmp/x")]);
    }

    #[test]
    fn quoting_survives_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(run_input(Path::new("/tmp/a b.sh")), "'/tmp/a b.sh'; exit\n");
    }

    #[test]
    fn control_characters_are_detected() {
        assert!(has_control_characters("/tmp/a\u{15}ls\r"));
        assert!(!has_control_characters("/tmp/ok"));
    }

    #[test]
    fn dock_drop_defaults_to_a_tab() {
        let env = Env { platform: Platform::MacOs, home: None, xdg_config_home: None, resources_dir: None };
        let tab = Config::from_str("", &env, ColorScheme::Dark);
        let window = Config::from_str("macos-dock-drop-behavior = new-window\n", &env, ColorScheme::Dark);
        assert_eq!(drop_behavior(&tab), DropBehavior::NewTab);
        assert_eq!(drop_behavior(&window), DropBehavior::NewWindow);
    }
}
