//! The Swift shim: Cocoa scripting objects and App Intents that call back into the
//! Rust application through `bridge`.

use gpui_kit::App;

#[cfg(angelly_shim)]
unsafe extern "C" {
    fn angelly_shim_install();
    fn angelly_shim_selftest_intents();
}

pub fn install() {
    #[cfg(angelly_shim)]
    unsafe {
        angelly_shim_install()
    };
}

/// Runs the App Intents self test when `ANGELLY_SELFTEST_INTENTS` is set. It starts once
/// the application has finished launching, because the intents talk to its windows.
pub fn selftest_intents(cx: &mut App) {
    #[cfg(angelly_shim)]
    if std::env::var_os("ANGELLY_SELFTEST_INTENTS").is_some() {
        cx.defer(|_| unsafe { angelly_shim_selftest_intents() });
    }
    #[cfg(not(angelly_shim))]
    let _ = cx;
}
