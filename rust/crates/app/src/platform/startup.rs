//! Process-wide setup that must happen before the application starts: where the bundled
//! resources are for the configuration and for child shells, and Cocoa defaults.

use std::ffi::OsStr;

use super::resources;

/// Call first thing in `main`, before any thread is started, because it sets environment
/// variables.
pub fn prepare() {
    export_resource("GHOSTTY_RESOURCES_DIR", "ghostty");
    export_resource("TERMINFO", "terminfo");
    #[cfg(target_os = "macos")]
    disable_press_and_hold();
}

/// Points `variable` at a bundled directory unless the user already set it.
fn export_resource(variable: &str, relative: &str) {
    if std::env::var_os(variable).is_some_and(|value| !value.is_empty()) {
        return;
    }
    if let Some(path) = resources::path(relative) {
        set_env(variable, path.as_os_str());
    }
}

fn set_env(variable: &str, value: &OsStr) {
    // The process is still single threaded when `prepare` runs.
    std::env::set_var(variable, value);
}

/// Holding a key should repeat it in a terminal, not open the accent picker.
#[cfg(target_os = "macos")]
fn disable_press_and_hold() {
    use objc2_foundation::{ns_string, NSUserDefaults};
    NSUserDefaults::standardUserDefaults().setBool_forKey(false, ns_string!("ApplePressAndHoldEnabled"));
}
