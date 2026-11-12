//! Panels: the command palette, sidebar quota footer, worktree popover and
//! sheet, Settings page and About window, the update pill and the modal alerts.
//!
//! A window hosts one [`Overlays`] entity that draws the modal and floating
//! panels over its content and emits [`OverlayEvent`] when a panel needs the
//! window to do something. Settings fills the host window; About opens separately.

pub(crate) mod about;
mod assets;
mod commands;
mod confirm;
mod default_commands;
mod format;
mod fuzzy;
mod host;
mod manage;
mod palette;
mod quota;
mod selftest;
mod settings;
pub(crate) mod ui;
mod update;
mod worktree;

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use angelly_core::git::GitService;
use gpui_kit::*;

pub use assets::AppAssets;
pub use commands::{commands_for, CommandAction, TabEntry};
pub use confirm::{ClipboardKind, ClipboardRequest, Confirm};
pub use update::{UpdateInstaller, UpdateModel};

pub(crate) use about::AppInfo;
use about::AboutView;
use commands::{build_options, PaletteSource};
use confirm::{ClipboardView, ConfirmView, Dismissed};
use manage::WorktreeManage;
use palette::{Palette, PaletteEvent};
use quota::{QuotaFooter, QuotaModel};
use settings::SettingsView;
use update::{UpdateEvent, UpdatePill};
use worktree::{WorktreeCreate, WorktreeEvent};

use crate::keybinds::{AppAction, Perform};
use crate::theme::Theme;

actions!(
    panels,
    [OpenSettings, ToggleCommandPalette, CheckForUpdates, ShowAbout, NewWorktree, ManageWorktrees]
);

/// Registers panel key bindings, state and global actions. Call once, after
/// the `Theme` global is set.
pub fn init(cx: &mut App) {
    ui::sync_component_theme(cx);

    let (quota_store, controller) = selftest::services();
    let quota = cx.new(|cx| QuotaModel::new(quota_store, cx));
    let update = cx.new(|cx| UpdateModel::new(controller, Arc::new(update::UnimplementedInstaller), cx));
    update.update(cx, |model, cx| model.start_automatic_checks(cx));
    cx.set_global(Services { quota, update, git: Arc::new(GitService::default()) });

    let (settings, palette) = if cfg!(target_os = "macos") { ("cmd-,", "cmd-shift-p") } else { ("ctrl-,", "ctrl-shift-p") };
    cx.bind_keys([KeyBinding::new(settings, OpenSettings, None), KeyBinding::new(palette, ToggleCommandPalette, None)]);
    cx.on_action(|_: &OpenSettings, cx| open_settings(cx));
    cx.on_action(|_: &ShowAbout, cx| open_about(cx));
    cx.on_action(|_: &CheckForUpdates, cx| check_for_updates(cx));
}

/// Installs the platform's update installer (unpack, swap bundle, relaunch).
pub fn set_update_installer(installer: Arc<dyn UpdateInstaller>, cx: &mut App) {
    let model = cx.global::<Services>().update.clone();
    model.update(cx, |model, _| model.set_installer(installer));
}

/// Models shared by every window.
pub struct Services {
    quota: Entity<QuotaModel>,
    update: Entity<UpdateModel>,
    git: Arc<GitService>,
}

impl Global for Services {}

/// Reloads the config into the `Theme` global and repaints every window.
pub fn reload_theme(cx: &mut App) {
    cx.set_global(Theme::load());
    ui::sync_component_theme(cx);
    crate::shortcuts::sync(cx);
    cx.refresh_windows();
}

pub fn check_for_updates(cx: &mut App) {
    let model = cx.global::<Services>().update.clone();
    model.update(cx, |model, cx| model.check(cx));
}

fn window_options(title: &str, width: f32, height: f32, transparent_titlebar: bool, cx: &App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.to_string().into()),
            appears_transparent: transparent_titlebar,
            traffic_light_position: transparent_titlebar.then(|| point(px(12.), px(12.))),
        }),
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    }
}

#[derive(Default)]
struct OpenWindows {
    about: Option<AnyWindowHandle>,
}

impl Global for OpenWindows {}

/// Brings the window forward when it is still open.
fn reactivate(handle: Option<AnyWindowHandle>, cx: &mut App) -> bool {
    handle.is_some_and(|handle| handle.update(cx, |_, window, _| window.activate_window()).is_ok())
}

pub fn open_settings(cx: &mut App) {
    let active = cx.active_window();
    // Global actions run while their window is borrowed; resolve it after dispatch.
    cx.defer(move |cx| {
        let host = active.and_then(|handle| {
            let root = handle.downcast::<gpui_kit::base::Root>()?;
            let workspace = root.read(cx).ok()?.view().clone().downcast::<crate::workspace::Workspace>().ok()?;
            Some(crate::app_host::Win { handle, workspace })
        })
        .or_else(|| crate::app_host::windows(cx).into_iter().next())
        .or_else(|| crate::app_host::open_window(&Default::default(), cx).ok());
        if let Some(host) = host {
            let _ = host.update(cx, |workspace, window, cx| {
                workspace.chrome.hover_card.dismiss(cx);
                let overlays = workspace.overlays(window, cx);
                overlays.update(cx, |overlays, cx| { overlays.show_settings(None, window, cx); });
                window.activate_window();
            });
        }
    });
}

pub fn open_about(cx: &mut App) {
    if !cx.has_global::<OpenWindows>() {
        cx.set_global(OpenWindows::default());
    }
    if reactivate(cx.global::<OpenWindows>().about, cx) {
        return;
    }
    let (width, height) = about::WINDOW_SIZE;
    let options = window_options("About Angelly", width, height, true, cx);
    let info = AppInfo::current();
    if let Ok((handle, _)) = gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| AboutView::new(info, window, cx))) {
        cx.global_mut::<OpenWindows>().about = Some(handle);
        cx.activate(true);
    }
}

/// What a panel needs to know about the window that hosts it.
#[derive(Default, Clone)]
pub struct HostInfo {
    pub tabs: Vec<TabEntry>,
    /// Working directory of the focused terminal.
    pub working_dir: Option<PathBuf>,
    /// Working directories of every open terminal.
    pub terminal_dirs: Vec<PathBuf>,
}

/// What a panel asks its window to do.
pub enum OverlayEvent {
    /// Run a Ghostty binding action such as `new_split:right` or `clear_screen`.
    Binding(String),
    FocusTab(usize),
    /// Open a new tab in this directory.
    OpenWorktree(PathBuf),
}

impl EventEmitter<OverlayEvent> for Overlays {}

enum Modal {
    Confirm(Entity<ConfirmView>),
    Clipboard(Entity<ClipboardView>),
}

enum Sheet {
    Manage(Entity<WorktreeManage>),
}

/// The floating and modal panels of one window.
pub struct Overlays {
    pub quota_footer: Entity<QuotaFooter>,
    pub update_pill: Entity<UpdatePill>,
    palette: Option<Entity<Palette>>,
    settings: Option<Entity<SettingsView>>,
    create: Option<(Entity<WorktreeCreate>, Point<Pixels>)>,
    sheet: Option<Sheet>,
    modals: Vec<Modal>,
    restore_focus: Option<WeakFocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl Overlays {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let (quota, update_model) = (services.quota.clone(), services.update.clone());
        let quota_footer = cx.new(|cx| QuotaFooter::new(quota, cx));
        let model = update_model.clone();
        let update_pill = cx.new(|cx| UpdatePill::new(model, cx));
        let restart = cx.subscribe_in(&update_model, window, |this, model, event: &UpdateEvent, window, cx| {
            if !window.is_window_active() {
                return;
            }
            let UpdateEvent::RestartRequired(release) = event;
            let model = model.clone();
            let request = Confirm::restart_required(release.as_ref(), move |_, cx| model.update(cx, |m, _| m.confirm()));
            this.confirm(request, window, cx);
        });
        let this = Self {
            quota_footer,
            update_pill,
            palette: None,
            settings: None,
            create: None,
            sheet: None,
            modals: Vec::new(),
            restore_focus: None,
            _subscriptions: vec![restart],
        };
        selftest::on_window_open(window, cx);
        this
    }

    /// Whether a question is waiting for an answer.
    pub fn is_confirming(&self) -> bool {
        self.modals.iter().any(|modal| matches!(modal, Modal::Confirm(_)))
    }

    pub(crate) fn settings_is_open(&self) -> bool {
        self.settings.is_some()
    }

    fn any_open(&self) -> bool {
        self.settings.is_some() || self.palette.is_some() || self.create.is_some() || self.sheet.is_some() || !self.modals.is_empty()
    }

    pub fn show_settings(&mut self, category: Option<settings::Category>, window: &mut Window, cx: &mut Context<Self>) -> Entity<SettingsView> {
        if let Some(view) = &self.settings {
            view.update(cx, |view, cx| {
                if let Some(category) = category { view.select_category(category, window, cx); }
                view.focus(window, cx);
            });
            return view.clone();
        }
        self.remember_focus(window, cx);
        self.palette = None;
        self.create = None;
        self.sheet = None;
        let view = cx.new(|cx| {
            let mut view = SettingsView::new(window, cx);
            if let Some(category) = category { view.select_category(category, window, cx); }
            view.focus(window, cx);
            view
        });
        cx.subscribe_in(&view, window, |this, _, _: &Dismissed, window, cx| {
            this.settings = None;
            this.restore_focus(window, cx);
            cx.notify();
        }).detach();
        self.settings = Some(view.clone());
        cx.notify();
        view
    }

    fn remember_focus(&mut self, window: &Window, cx: &App) {
        if !self.any_open() {
            self.restore_focus = window.focused(cx).map(|handle| handle.downgrade());
        }
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.any_open() {
            return;
        }
        if let Some(handle) = self.restore_focus.take().and_then(|weak| weak.upgrade()) {
            window.focus(&handle, cx);
        }
    }

    /// Opens or closes the command palette.
    pub fn toggle_palette(&mut self, host: HostInfo, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.restore_focus(window, cx);
            cx.notify();
            return;
        }
        self.remember_focus(window, cx);
        let update_state = cx.global::<Services>().update.read(cx).state();
        let home = std::env::var("HOME").ok();
        let options = {
            let theme = cx.global::<Theme>();
            build_options(&PaletteSource { config: &theme.config, tabs: &host.tabs, update: &update_state, home: home.as_deref() })
        };
        let palette = cx.new(|cx| Palette::new(options, window, cx));
        self._subscriptions.push(cx.subscribe_in(&palette, window, |this, _, event: &PaletteEvent, window, cx| match event {
            PaletteEvent::Dismissed => {
                this.palette = None;
                this.restore_focus(window, cx);
                cx.notify();
            }
            PaletteEvent::Run(action) => this.run(action.clone(), window, cx),
        }));
        self.palette = Some(palette);
        cx.notify();
    }

    fn run(&mut self, action: CommandAction, window: &mut Window, cx: &mut Context<Self>) {
        let update = cx.global::<Services>().update.clone();
        match action {
            CommandAction::FocusTab(index) => cx.emit(OverlayEvent::FocusTab(index)),
            CommandAction::InstallUpdate => update.update(cx, |model, _| model.confirm()),
            CommandAction::CancelUpdate => update.update(cx, |model, _| model.cancel()),
            CommandAction::Binding(action) => match action.as_str() {
                "check_for_updates" => update.update(cx, |model, cx| model.check(cx)),
                "open_config" => { self.show_settings(None, window, cx); },
                _ => cx.emit(OverlayEvent::Binding(action)),
            },
        }
        let _ = window;
    }

    /// Shows the "New Worktree" popover, anchored at `anchor` (window
    /// coordinates) or at the top left under the titlebar.
    pub fn new_worktree(&mut self, host: HostInfo, anchor: Option<Point<Pixels>>, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_focus(window, cx);
        let git = cx.global::<Services>().git.clone();
        let directory = host.working_dir;
        let view = cx.new(|cx| WorktreeCreate::new(git, directory, window, cx));
        self._subscriptions.push(cx.subscribe_in(&view, window, |this, _, event: &WorktreeEvent, window, cx| {
            this.create = None;
            this.restore_focus(window, cx);
            if let WorktreeEvent::Open(path) = event {
                cx.emit(OverlayEvent::OpenWorktree(path.clone()));
            }
            cx.notify();
        }));
        self.create = Some((view, anchor.unwrap_or_else(|| point(px(12.), px(40.)))));
        cx.notify();
    }

    /// Shows the "Manage Worktrees" sheet.
    pub fn manage_worktrees(&mut self, host: HostInfo, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_focus(window, cx);
        let git = cx.global::<Services>().git.clone();
        let view = cx.new(|cx| WorktreeManage::new(git, host.working_dir, host.terminal_dirs, window, cx));
        self._subscriptions.push(cx.subscribe_in(&view, window, |this, _, event: &WorktreeEvent, window, cx| {
            this.sheet = None;
            this.restore_focus(window, cx);
            if let WorktreeEvent::Open(path) = event {
                cx.emit(OverlayEvent::OpenWorktree(path.clone()));
            }
            cx.notify();
        }));
        self.sheet = Some(Sheet::Manage(view));
        cx.notify();
    }

    /// Asks a question; `request.on_confirm` runs only if the user agrees.
    pub fn confirm(&mut self, request: Confirm, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_focus(window, cx);
        let view = cx.new(|cx| ConfirmView::new(request, window, cx));
        self._subscriptions.push(cx.subscribe_in(&view, window, |this, view, _: &Dismissed, window, cx| {
            this.modals.retain(|m| !matches!(m, Modal::Confirm(v) if v == view));
            this.restore_focus(window, cx);
            cx.notify();
        }));
        self.modals.push(Modal::Confirm(view));
        cx.notify();
    }

    /// Shows the clipboard confirmation sheet for an unsafe paste or an
    /// OSC 52 / Kitty clipboard request. The request's `completion` always
    /// runs exactly once.
    pub fn clipboard(&mut self, request: ClipboardRequest, window: &mut Window, cx: &mut Context<Self>) {
        self.remember_focus(window, cx);
        let view = cx.new(|cx| ClipboardView::new(request, window, cx));
        self._subscriptions.push(cx.subscribe_in(&view, window, |this, view, _: &Dismissed, window, cx| {
            this.modals.retain(|m| !matches!(m, Modal::Clipboard(v) if v == view));
            this.restore_focus(window, cx);
            cx.notify();
        }));
        self.modals.push(Modal::Clipboard(view));
        cx.notify();
    }

    /// Opens the update pill's popover.
    pub fn show_update_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.update_pill.update(cx, |pill, cx| pill.open_popover(window, cx));
    }
}

impl Render for Overlays {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let sheet_top = crate::window_chrome::Spec::of(cx).titlebar_height();
        let mut layer = div().absolute().inset_0().font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT));

        if let Some(settings) = &self.settings {
            layer = layer.child(div().absolute().inset_0().occlude().child(settings.clone()));
        }

        if let Some(palette) = &self.palette {
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(viewport.height * 0.05)
                    .px(px(16.))
                    .child(palette.clone()),
            );
        }

        if let Some((create, anchor)) = &self.create {
            let dismiss = create.clone();
            layer = layer.child(
                div()
                    .absolute()
                    .inset_0()
                    .occlude()
                    .on_any_mouse_down(cx.listener(move |_, _, _, cx| dismiss.update(cx, |view, cx| view.dismiss(cx))))
                    .child(
                        deferred(anchored().position(*anchor).snap_to_window_with_margin(px(8.)).child(create.clone()))
                            .with_priority(2),
                    ),
            );
        }

        if let Some(Sheet::Manage(manage)) = &self.sheet {
            layer = layer.child(scrim(0.12, 0.).justify_center().child(manage.clone()));
        }

        if let Some(modal) = self.modals.last() {
            let content = match modal {
                Modal::Confirm(view) => view.clone().into_any_element(),
                Modal::Clipboard(view) => view.clone().into_any_element(),
            };
            layer = layer.child(scrim(0.18, sheet_top).child(content));
        }

        layer.child(
            div().absolute().right(px(9.)).bottom(px(9.)).child(self.update_pill.clone()),
        )
    }
}

/// A window-sized blocking layer that lets a sheet hang from the top centre, below the
/// titlebar like a native sheet.
fn scrim(dim: f32, top: f32) -> Div {
    div()
        .absolute()
        .inset_0()
        .pt(px(top))
        .occlude()
        .bg(black().opacity(dim))
        .flex()
        .flex_col()
        .items_center()
}

/// Wires the panel actions onto a window's root element: the command palette
/// and the worktree actions. `host` describes the window when one opens.
pub fn attach(root: Div, overlays: &Entity<Overlays>, host: impl Fn(&App) -> HostInfo + 'static) -> Div {
    let settings = {
        let overlays = overlays.clone();
        move |_: &OpenSettings, window: &mut Window, cx: &mut App| {
            overlays.update(cx, |o, cx| { o.show_settings(None, window, cx); });
        }
    };
    let host = Rc::new(host);
    let toggle = {
        let (overlays, host) = (overlays.clone(), host.clone());
        Rc::new(move |window: &mut Window, cx: &mut App| {
            let info = host(cx);
            overlays.update(cx, |o, cx| o.toggle_palette(info, window, cx));
        })
    };
    let create = {
        let (overlays, host) = (overlays.clone(), host.clone());
        move |_: &NewWorktree, window: &mut Window, cx: &mut App| {
            let info = host(cx);
            overlays.update(cx, |o, cx| o.new_worktree(info, None, window, cx));
        }
    };
    let manage = {
        let (overlays, host) = (overlays.clone(), host);
        move |_: &ManageWorktrees, window: &mut Window, cx: &mut App| {
            let info = host(cx);
            overlays.update(cx, |o, cx| o.manage_worktrees(info, window, cx));
        }
    };
    let palette = {
        let toggle = toggle.clone();
        move |_: &ToggleCommandPalette, window: &mut Window, cx: &mut App| toggle(window, cx)
    };
    let performed = move |Perform(action): &Perform, window: &mut Window, cx: &mut App| match action {
        AppAction::ToggleCommandPalette => toggle(window, cx),
        AppAction::CheckForUpdates => check_for_updates(cx),
        _ => cx.propagate(),
    };
    root.on_action(settings).on_action(palette).on_action(create).on_action(manage).on_action(performed)
}
