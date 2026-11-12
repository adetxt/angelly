//! "Float on Top": keeps the key window above the windows of other applications.

use gpui_kit::App;

#[cfg(target_os = "macos")]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSFloatingWindowLevel, NSNormalWindowLevel};

    pub fn is_floating() -> bool {
        let Some(mtm) = MainThreadMarker::new() else { return false };
        NSApplication::sharedApplication(mtm)
            .keyWindow()
            .is_some_and(|window| window.level() == NSFloatingWindowLevel)
    }

    pub fn toggle() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() else { return };
        let level = if window.level() == NSFloatingWindowLevel { NSNormalWindowLevel } else { NSFloatingWindowLevel };
        window.setLevel(level);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_floating() -> bool {
        false
    }

    pub fn toggle() {}
}

pub fn is_floating() -> bool {
    imp::is_floating()
}

/// Flips the key window's level and refreshes the menu check mark.
pub fn toggle(cx: &mut App) {
    imp::toggle();
    super::menus::install(cx);
}
