//! Locating bundled resources: icons, shell integration, terminfo and themes.
//!
//! Inside `Angelly.app` they sit in `Contents/Resources`, in an installed Linux package
//! in `<prefix>/share/angelly`; a development build finds them through
//! `ANGELLY_RESOURCES` or next to the executable.

use std::path::PathBuf;

pub fn dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ANGELLY_RESOURCES").map(PathBuf::from) {
        return Some(dir);
    }
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    let bundled = parent.parent()?.join("Resources");
    if bundled.is_dir() {
        return Some(bundled);
    }
    let installed = parent.parent()?.join("share/angelly");
    if installed.is_dir() {
        return Some(installed);
    }
    let beside = parent.join("resources");
    beside.is_dir().then_some(beside)
}

pub fn path(relative: &str) -> Option<PathBuf> {
    let path = dir()?.join(relative);
    path.exists().then_some(path)
}
