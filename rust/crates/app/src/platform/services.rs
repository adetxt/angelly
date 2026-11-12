//! The "New Angelly Tab Here" and "New Angelly Window Here" Services (spec 16.7).
//!
//! The Info.plist declares both entries; the selectors below receive the pasteboard.
//! Each file is reduced to its directory, duplicates collapse, and one window or tab
//! opens per directory.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::command::{self, Command};
use super::runtime;

pub const NO_PATHS: &str = "Could not load any text from the clipboard.";

/// Reduces paths to the directories to open: a file stands for its parent, and each
/// directory appears once, in first-seen order.
pub fn directories(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    paths
        .into_iter()
        .map(|path| if path.is_dir() { path } else { path.parent().map(Path::to_path_buf).unwrap_or(path) })
        .map(|path| strip_trailing_separator(&path))
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn strip_trailing_separator(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    if trimmed.is_empty() {
        path.to_path_buf()
    } else {
        PathBuf::from(trimmed)
    }
}

/// Paths written as plain text: one per line, either absolute paths or `file://` URLs.
pub fn paths_from_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| match line.strip_prefix("file://") {
            Some(rest) => Some(PathBuf::from(percent_decode(rest))),
            None => line.starts_with('/').then(|| PathBuf::from(line)),
        })
        .collect()
}

pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(value) = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn open(paths: Vec<PathBuf>, tabs: bool) -> Result<(), &'static str> {
    let directories = directories(paths);
    if directories.is_empty() {
        return Err(NO_PATHS);
    }
    runtime::with_app(|cx| command::perform(Command::OpenDirectories { paths: directories, tabs }, cx));
    Ok(())
}

#[cfg(target_os = "macos")]
pub use provider::install;

#[cfg(not(target_os = "macos"))]
pub fn install() {}

#[cfg(target_os = "macos")]
mod provider {
    use std::path::PathBuf;

    use objc2::rc::Retained;
    use objc2::{define_class, msg_send, AllocAnyThread};
    use objc2_app_kit::{NSApplication, NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{MainThreadMarker, NSArray, NSObject, NSString};

    use super::{open, paths_from_text};

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "AngellyServiceProvider"]
        struct ServiceProvider;

        impl ServiceProvider {
            #[unsafe(method(openTab:userData:error:))]
            fn open_tab(&self, pasteboard: &NSPasteboard, _user_data: Option<&NSString>, error: *mut *mut NSString) {
                serve(pasteboard, true, error);
            }

            #[unsafe(method(openWindow:userData:error:))]
            fn open_window(&self, pasteboard: &NSPasteboard, _user_data: Option<&NSString>, error: *mut *mut NSString) {
                serve(pasteboard, false, error);
            }
        }
    );

    fn pasteboard_paths(pasteboard: &NSPasteboard) -> Vec<PathBuf> {
        let list = pasteboard.propertyListForType(&NSString::from_str("NSFilenamesPboardType"));
        if let Some(list) = list {
            if let Ok(array) = list.downcast::<NSArray>() {
                let paths: Vec<PathBuf> = array
                    .iter()
                    .filter_map(|item| item.downcast::<NSString>().ok())
                    .map(|s| PathBuf::from(s.to_string()))
                    .collect();
                if !paths.is_empty() {
                    return paths;
                }
            }
        }
        pasteboard
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|text| paths_from_text(&text.to_string()))
            .unwrap_or_default()
    }

    fn serve(pasteboard: &NSPasteboard, tabs: bool, error: *mut *mut NSString) {
        if let Err(message) = open(pasteboard_paths(pasteboard), tabs) {
            if !error.is_null() {
                unsafe { *error = Retained::autorelease_ptr(NSString::from_str(message)) };
            }
        }
    }

    /// Registers the provider with the application. The Services entries themselves come
    /// from `NSServices` in Info.plist.
    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let provider: Retained<ServiceProvider> = unsafe { msg_send![ServiceProvider::alloc(), init] };
        let app = NSApplication::sharedApplication(mtm);
        unsafe { app.setServicesProvider(Some(&provider)) };
        // The application keeps only a weak reference, so the provider lives for the process.
        std::mem::forget(provider);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_collapse_to_their_directory_once() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let other = dir.path().join("sub");
        std::fs::create_dir(&other).unwrap();
        let result = directories([file.clone(), dir.path().join("b.txt"), other.clone(), PathBuf::from(format!("{}/", other.display()))]);
        assert_eq!(result, vec![dir.path().to_path_buf(), other]);
    }

    #[test]
    fn text_paths_accept_urls_and_absolute_paths() {
        let paths = paths_from_text("/tmp/a b\nfile:///tmp/c%20d\nrelative\n\n");
        assert_eq!(paths, vec![PathBuf::from("/tmp/a b"), PathBuf::from("/tmp/c d")]);
    }

    #[test]
    fn nothing_to_open_is_an_error() {
        assert_eq!(open(Vec::new(), true), Err(NO_PATHS));
    }
}
