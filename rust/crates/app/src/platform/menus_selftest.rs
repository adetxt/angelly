//! Test hook behind `ANGELLY_SELFTEST_MENUS`: prints the native menu bar, one line per item
//! with its key equivalent, so a script can check what the menus really contain.

use gpui_kit::App;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSMenu};
use objc2_foundation::NSString;

pub fn run(cx: &mut App) {
    if std::env::var_os("ANGELLY_SELFTEST_MENUS").is_none() {
        return;
    }
    cx.defer(|_| {
        let Some(mtm) = MainThreadMarker::new() else { return };
        match NSApplication::sharedApplication(mtm).mainMenu() {
            Some(menu) => dump(&menu, 0),
            None => println!("menus selftest: no main menu"),
        }
    });
}

fn dump(menu: &NSMenu, depth: usize) {
    for item in menu.itemArray().iter() {
        let indent = "  ".repeat(depth);
        if item.isSeparatorItem() {
            println!("menus selftest: {indent}---");
            continue;
        }
        let key = item.keyEquivalent();
        let mask = item.keyEquivalentModifierMask().0;
        let flags = if key.is_empty() { String::new() } else { format!(" [{}:{mask:x}]", escape(&key)) };
        let state = if item.state() == 1 { " (checked)" } else { "" };
        let disabled = if item.isEnabled() { "" } else { " (disabled)" };
        println!("menus selftest: {indent}{}{flags}{state}{disabled}", item.title());
        if let Some(submenu) = item.submenu() {
            dump(&submenu, depth + 1);
        }
    }
}

fn escape(key: &NSString) -> String {
    key.to_string().escape_default().to_string()
}
