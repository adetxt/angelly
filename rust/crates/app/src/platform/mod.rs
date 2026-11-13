//! Integration with the operating system: menu bar, dock, services, scripting,
//! global shortcuts and the updater's installer. One module per feature, each
//! compiled only on the platforms that have it.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

pub(crate) mod actions;
pub mod automation;
pub(crate) mod app_icon;
#[cfg(target_os = "macos")]
mod bridge;
pub(crate) mod command;
pub(crate) mod float_on_top;
mod default_terminal;
#[cfg(not(target_os = "macos"))]
mod desktop_notify;
mod hotkey;
mod ipc;
pub(crate) mod open;
pub mod menus;
#[cfg(target_os = "macos")]
mod menus_selftest;
#[cfg(target_os = "macos")]
mod notifications;
pub(crate) mod quick_terminal;
pub(crate) mod window_controls;
#[cfg(unix)]
mod reload_signal;
mod resources;
mod runtime;
pub(crate) mod popup_surface;
mod services;
mod shim;
pub mod secure_input;
mod startup;
mod state;
#[cfg(target_os = "macos")]
pub mod updater;
mod visibility;
mod window_actions;

use std::rc::Rc;

use gpui_kit::App;

use crate::theme::Theme;

pub use command::{Command, Host};

/// The Wayland app id and X11 window class; the desktop entry's name and its
/// `StartupWMClass` must match it.
pub const APP_ID: &str = "com.adetxt.angelly";

/// Handles `angelly msg ...` on the command line. Returns the exit code when the process
/// was such an invocation, in which case the application must not start.
pub fn run_cli() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    ipc::run_cli(&args)
}

/// Prepares the process environment. Call first thing in `main`.
pub fn prepare() {
    startup::prepare();
}

/// Registers the platform features. Call once from the application's startup closure,
/// after the keymap and globals the views need exist.
pub fn init(cx: &mut App, host: Rc<dyn Host>) {
    cx.set_global(command::HostGlobal(host));
    runtime::install(cx);
    secure_input::init(cx);
    menus::init(cx);
    #[cfg(target_os = "macos")]
    menus_selftest::run(cx);
    window_actions::register(cx);
    #[cfg(unix)]
    reload_signal::install(cx);
    ipc::start(cx);
    #[cfg(not(target_os = "macos"))]
    desktop_notify::install(cx);
    cx.observe_global::<Theme>(|cx| {
        apply_config(cx);
        menus::install(cx);
    })
    .detach();
    services::install();
    shim::install();
    shim::selftest_intents(cx);
    apply_config(cx);
    open::flush_pending(cx);
}

/// Applies the parts of the configuration the platform layer owns: the app icon and
/// the global keybinds. Call it again after the configuration reloads.
pub fn apply_config(cx: &mut App) {
    app_icon::apply(cx);
    hotkey::reload(cx);
}

/// Handler for `Application::on_open_urls`: folders and files dropped on the Dock icon,
/// opened from Finder or passed to `open -a`.
pub fn open_urls(urls: Vec<String>) {
    open::open_urls(urls);
}
