//! Window state that outlives the process: the tabs restored at launch and the
//! history of recently closed windows.
//!
//! Both use the macOS app's JSON layouts (`RestorableState`, `ClosedWindowSnapshot`).

use std::cell::RefCell;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use angelly_core::config::{Env, WindowSaveState};
use angelly_core::persist::{
    closed_window_title, data_dir, window_title, ClosedTab, ClosedWindowRecoveryContext, ClosedWindowSnapshot,
    ClosedWindowStore, GroupSnapshot, Rect, RestorableState, SurfaceState,
};
use angelly_core::persist::SplitTree as Tree;
use gpui_kit::*;
use uuid::Uuid;

use crate::splits::Splits;
use crate::tabs::ReopenClosedWindow;
use crate::theme::Theme;
use crate::window_chrome::Spec;
use crate::workspace::{Tab, Workspace};

const SESSIONS_DIRECTORY: &str = "sessions";
const SAVE_DELAY: Duration = Duration::from_millis(500);

/// Where state is saved. `None` for self tests, which must not touch the user's data:
/// any `ANGELLY_SELFTEST*` variable disables it unless `ANGELLY_PERSIST` is set.
pub fn data_directory() -> Option<PathBuf> {
    let selftest = std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("ANGELLY_SELFTEST"));
    if selftest && std::env::var_os("ANGELLY_PERSIST").is_none() {
        return None;
    }
    data_dir(&Env::from_process())
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        builder.mode(0o700);
        options.mode(0o600);
    }
    builder.create(directory)?;
    let temporary = directory.join(format!(".{}.tmp", Uuid::new_v4()));
    let result = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// The recently closed windows, shared by every window of the app.
#[derive(Clone)]
pub struct ClosedWindows(Rc<RefCell<ClosedWindowStore>>);

impl Global for ClosedWindows {}

impl ClosedWindows {
    fn get(cx: &mut App) -> Option<ClosedWindows> {
        if let Some(existing) = cx.try_global::<ClosedWindows>() {
            return Some(existing.clone());
        }
        let directory = data_directory()?;
        let mut store = ClosedWindowStore::new(directory);
        if cx.global::<Theme>().config.window_save_state() != WindowSaveState::Never {
            let _ = store.set_persistence_enabled(true);
        }
        let windows = ClosedWindows(Rc::new(RefCell::new(store)));
        cx.set_global(windows.clone());
        windows.publish(cx);
        Some(windows)
    }

    /// Updates the menu bar's "Recently Closed Windows" list.
    fn publish(&self, cx: &mut App) {
        let store = self.0.borrow();
        let entries = store
            .entries()
            .iter()
            .map(|snapshot| format!("{} · {}", snapshot.title, tab_count(snapshot.tabs.len())))
            .collect();
        crate::platform::menus::set_recently_closed(entries, store.is_persistence_enabled(), cx);
    }
}

fn tab_count(count: usize) -> String {
    if count == 1 {
        "1 tab".to_string()
    } else {
        format!("{count} tabs")
    }
}

/// Starts the recently closed windows history and handles the reopen actions.
pub fn init(cx: &mut App) {
    ClosedWindows::get(cx);
    cx.on_action(|_: &ReopenClosedWindow, cx| reopen_closed_window(None, cx));
}

/// Reopens the recently closed window at `index` (newest first; the newest when `None`)
/// in a new window.
pub fn reopen_closed_window(index: Option<usize>, cx: &mut App) {
    let Some(windows) = ClosedWindows::get(cx) else { return };
    let snapshot = windows.0.borrow().entries().get(index.unwrap_or(0)).map(|s| (*s).clone());
    let Some(snapshot) = snapshot else { return };
    let bounds = bounds_of(&snapshot.frame);
    let id = snapshot.id;
    let opened = open_window(Some(bounds), cx, move |window, cx| {
        let mut context = ClosedWindowRecoveryContext::new();
        cx.new(|cx| Workspace::from_snapshot(&snapshot, Some(&mut context), window, cx))
    });
    if opened.is_ok() {
        let _ = windows.0.borrow_mut().consume(id);
        windows.publish(cx);
    }
}

pub fn clear_closed_windows(cx: &mut App) {
    if let Some(windows) = ClosedWindows::get(cx) {
        let _ = windows.0.borrow_mut().clear();
        windows.publish(cx);
    }
}

/// Opens a terminal window, at `bounds` when given.
pub fn open_window(
    bounds: Option<Bounds<Pixels>>,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<Workspace>,
) -> Result<(AnyWindowHandle, Entity<Workspace>)> {
    let config_sized = bounds.is_none();
    let frame = bounds.unwrap_or_else(|| Bounds::centered(None, size(px(1100.), px(720.)), cx));
    let spec = Spec::of(cx);
    let options = WindowOptions {
        window_bounds: Some(spec.window_bounds(frame)),
        titlebar: spec.titlebar("Angelly"),
        app_id: Some(crate::platform::APP_ID.into()),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, move |window, cx| {
        let workspace = build(window, cx);
        workspace.update(cx, |workspace, _| workspace.chrome.sized_by_config = config_sized);
        workspace
    })
}

fn bounds_of(frame: &Rect) -> Bounds<Pixels> {
    Bounds::new(point(px(frame.x as f32), px(frame.y as f32)), size(px(frame.width as f32), px(frame.height as f32)))
}

fn rect_of(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        x: f64::from(f32::from(bounds.origin.x)),
        y: f64::from(f32::from(bounds.origin.y)),
        width: f64::from(f32::from(bounds.size.width)),
        height: f64::from(f32::from(bounds.size.height)),
    }
}

/// One window of the last session and the file that keeps it.
pub struct SavedWindow {
    path: PathBuf,
    snapshot: ClosedWindowSnapshot,
}

impl SavedWindow {
    /// The saved frame, when it is still on a display.
    pub fn bounds(&self, cx: &App) -> Option<Bounds<Pixels>> {
        let bounds = bounds_of(&self.snapshot.frame);
        cx.displays().iter().any(|display| display.bounds().intersects(&bounds)).then_some(bounds)
    }
}

fn sessions_directory() -> Option<PathBuf> {
    data_directory().map(|dir| dir.join(SESSIONS_DIRECTORY))
}

/// The file a new window keeps its session in. Every window has its own.
pub fn new_session_path() -> Option<PathBuf> {
    sessions_directory().map(|dir| dir.join(format!("{}.json", Uuid::new_v4())))
}

/// The windows of the last session, the most recently saved last. Nothing is restored
/// with `window-save-state = never`, and the files of an earlier run are dropped.
pub fn saved_windows(cx: &App) -> Vec<SavedWindow> {
    let Some(directory) = sessions_directory() else { return Vec::new() };
    let files: Vec<PathBuf> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect();
    if cx.global::<Theme>().config.window_save_state() == WindowSaveState::Never {
        files.iter().for_each(|path| drop(fs::remove_file(path)));
        return Vec::new();
    }
    let mut windows: Vec<SavedWindow> = files
        .into_iter()
        .filter_map(|path| {
            let snapshot: ClosedWindowSnapshot = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            snapshot.validate().ok()?;
            Some(SavedWindow { path, snapshot })
        })
        .collect();
    windows.sort_by_key(|window| window.snapshot.closed_at);
    windows
}

fn is_directory(path: &str) -> bool {
    Path::new(path).is_dir()
}

impl Workspace {
    /// Restores the tabs of a window of the last session and keeps writing to its file.
    pub fn restore_saved(&mut self, saved: SavedWindow, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.session_path = Some(saved.path);
        self.restore_tabs(&saved.snapshot, None, window, cx);
    }

    /// A workspace holding the tabs of a saved window.
    pub fn from_snapshot(
        snapshot: &ClosedWindowSnapshot,
        context: Option<&mut ClosedWindowRecoveryContext>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut this = Workspace::empty(cx);
        this.restore_tabs(snapshot, context, window, cx);
        if this.tabs.is_empty() {
            this.add_tab(None, window, cx);
        }
        this.start_git_polling(cx);
        this.install_window_hooks(window, cx);
        this
    }

    fn restore_tabs(
        &mut self,
        snapshot: &ClosedWindowSnapshot,
        mut context: Option<&mut ClosedWindowRecoveryContext>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for saved in &snapshot.tabs {
            let Ok(mut state) = saved.decode() else { continue };
            if let Some(context) = context.as_deref_mut() {
                state = context.recover(state, is_directory);
            }
            let focused = state.focused_surface.clone();
            let (view, splits) = Splits::restore(
                &state.surface_tree,
                |surface| surface.pwd.clone().filter(|p| is_directory(p)).map(PathBuf::from),
                |surface| focused.as_deref() == Some(surface.id_string().as_str()),
                window,
                cx,
            );
            let group = state.sidebar_group.map(|snapshot: GroupSnapshot| {
                let id = snapshot.id;
                self.chrome.groups.restore(snapshot);
                id
            });
            let id = self.next_tab_id();
            self.tabs.push(Tab {
                id,
                view,
                splits,
                color: state.tab_color.unwrap_or_default(),
                title_override: state.title_override,
                group,
            });
        }
        if !self.tabs.is_empty() {
            let selected = usize::try_from(snapshot.selected_tab_index).unwrap_or(0);
            self.select(selected.min(self.tabs.len() - 1), window, cx);
        }
    }

    /// The saved state of every tab.
    pub(crate) fn snapshot(&self, cx: &App) -> Option<ClosedWindowSnapshot> {
        let tabs: Vec<ClosedTab> = self
            .tabs
            .iter()
            .map(|tab| ClosedTab::encode(&self.tab_state(tab, cx)))
            .collect::<Result<_, _>>()
            .ok()?;
        let title = self.tab_title(&self.tabs[self.selected], cx);
        let branch = self.chrome.git.status_for(self.tabs[self.selected].id).map(|s| s.titlebar_branch());
        let title = closed_window_title(
            self.tabs[self.selected].title_override.as_deref(),
            &window_title(&title, branch.as_deref()),
        );
        Some(ClosedWindowSnapshot::new(&title, rect_of(self.chrome.window_bounds), self.selected, tabs, SystemTime::now()))
    }

    fn tab_state(&self, tab: &Tab, cx: &App) -> RestorableState<SurfaceState> {
        let focused = tab.view.clone();
        let mut focused_id = None;
        let tree: Tree<SurfaceState> = tab.splits.map_panes(|pane| {
            let mut surface = SurfaceState::new(Uuid::new_v4());
            let view = pane.read(cx);
            surface.pwd = view.cwd().map(|p| p.to_string_lossy().into_owned());
            surface.title = Some(view.title());
            if *pane == focused {
                focused_id = Some(surface.id_string());
            }
            surface
        });
        let mut state = RestorableState::new(tree);
        state.focused_surface = focused_id;
        state.tab_color = Some(tab.color).filter(|c| *c != angelly_core::persist::TerminalTabColor::None);
        state.title_override = tab.title_override.clone();
        state.sidebar_group = self.chrome.groups.snapshot(tab.group);
        state
    }

    /// Saves the session shortly after the last change.
    pub fn save_session(&mut self, cx: &mut Context<Self>) {
        if self.chrome.session_path.is_none() {
            return;
        }
        self.chrome.save_generation += 1;
        let generation = self.chrome.save_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            this.update(cx, |this, cx| {
                if this.chrome.save_generation == generation {
                    this.write_session(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Writes the session now. Quitting keeps it for the next launch.
    pub fn write_session(&self, cx: &App) {
        let Some(path) = self.chrome.session_path.clone() else { return };
        let Some(snapshot) = self.snapshot(cx) else { return };
        if let Ok(bytes) = serde_json::to_vec(&snapshot) {
            let _ = write_private(&path, &bytes);
        }
    }

    /// A deliberate close: the window goes into the recently closed windows and the
    /// next launch starts fresh.
    pub fn record_closed_window(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.chrome.session_path.clone() else { return };
        let _ = fs::remove_file(path);
        self.chrome.save_generation += 1;
        let Some(snapshot) = self.snapshot(cx) else { return };
        if let Some(windows) = ClosedWindows::get(cx) {
            let _ = windows.0.borrow_mut().append(snapshot);
            windows.publish(cx);
        }
    }
}
