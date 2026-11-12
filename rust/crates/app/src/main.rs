mod app_host;
mod assets;
mod platform;
mod quick_terminal;
mod workspace_files;
mod sidebar;
mod splits;
mod tab_ops;
mod titlebar;
mod tabs;
mod tab_menus;
mod tab_dnd;
mod tab_colors;
mod tab_bar;
mod tab_popover;
mod session;
mod shortcuts;
mod open_with;
mod panels;
mod prefs;
mod keybinds;
mod middle_text;
mod menu;
mod git_status;
mod chrome;
mod dock_bell;
mod terminal_view;
mod theme;
mod window_chrome;
mod workspace;

use gpui_kit::*;
use theme::Theme;
use workspace::{ToggleSidebar, Workspace};

fn main() {
    if let Some(code) = platform::run_cli() {
        std::process::exit(code);
    }
    platform::prepare();
    let app = gpui_kit::application().with_assets(panels::AppAssets);
    app.on_open_urls(platform::open_urls);
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            platform::command::perform(platform::command::Command::NewWindow, cx);
        }
    });
    app.run(|cx| {
        gpui_kit::init(cx);
        cx.set_global(Theme::load());
        prefs::Prefs::init(cx);
        panels::init(cx);
        cx.bind_keys([KeyBinding::new("ctrl-cmd-s", ToggleSidebar, Some("Workspace"))]);
        cx.bind_keys(shortcuts::terminal_input_bindings());
        session::init(cx);
        app_host::install(cx);
        shortcuts::sync(cx);
        dock_bell::install(cx);
        let mut saved = session::saved_windows(cx).into_iter().map(Some).collect::<Vec<_>>();
        if saved.is_empty() {
            saved.push(None);
        }
        for (index, saved) in saved.into_iter().enumerate() {
            let bounds = saved.as_ref().and_then(|saved| saved.bounds(cx));
            session::open_window(bounds, cx, move |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(saved, window, cx));
                if index == 0 {
                    workspace.update(cx, |workspace, cx| splits::selftest(workspace, window, cx));
                    workspace.update(cx, |workspace, cx| tabs::selftest(workspace, window, cx));
                }
                workspace
            })
            .expect("open window");
        }
        cx.activate(true);
        if std::env::var_os("ANGELLY_SELFTEST_QUICK").is_some() {
            quick_terminal::selftest(cx);
        }
    });
}
