//! "Show/Hide All Terminals": hides the app when it is frontmost, otherwise brings
//! every window forward.

use gpui_kit::App;

#[cfg(target_os = "macos")]
pub fn toggle(cx: &mut App) {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    if app.isActive() && !app.isHidden() {
        cx.hide();
    } else {
        app.unhide(None);
        cx.activate(true);
    }
}

#[cfg(not(target_os = "macos"))]
pub fn toggle(cx: &mut App) {
    cx.activate(true);
}
