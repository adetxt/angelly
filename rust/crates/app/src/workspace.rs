use std::path::PathBuf;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use angelly_core::persist::{window_title, TerminalTabColor};
use uuid::Uuid;

use crate::chrome::{ResizeDrag, SidebarDragging, TabChrome};
use crate::splits::{self, Splits};
use crate::panels::{self, Overlays};
use crate::prefs::Prefs;
use crate::session::SavedWindow;
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::window_chrome::Spec;
use crate::{sidebar, tab_bar, tabs, titlebar};

pub type TabId = u64;

actions!(
    workspace,
    [NewTab, CloseTab, NextTab, PreviousTab, ToggleSidebar]
);

pub const TITLEBAR_HEIGHT: f32 = 32.0;
pub const SIDEBAR_MIN_WIDTH: f32 = 160.0;
pub const SIDEBAR_MAX_WIDTH: f32 = 480.0;

pub struct Tab {
    pub id: TabId,
    /// The focused pane of the tab: the one its title, cwd and new splits come from.
    pub view: Entity<TerminalView>,
    pub splits: Splits,
    pub color: TerminalTabColor,
    pub title_override: Option<String>,
    pub group: Option<Uuid>,
}

pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub selected: usize,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub chrome: TabChrome,
    pub(crate) overlays: Option<Entity<Overlays>>,
}

impl Workspace {
    /// A workspace restoring a window of the last session, or starting with one tab.
    pub fn new(saved: Option<SavedWindow>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Workspace::empty(cx);
        if let Some(saved) = saved {
            this.restore_saved(saved, window, cx);
        }
        if this.tabs.is_empty() {
            this.add_tab(None, window, cx);
        }
        this.start_git_polling(cx);
        this.install_window_hooks(window, cx);
        this
    }

    /// A workspace without tabs, with the saved sidebar state.
    pub fn empty(cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| {
            this.chrome.hover_card.dismiss(cx);
            if this.chrome.resize.take().is_some() {
                SidebarDragging::set(false, cx);
            }
        })
        .detach();
        let chrome = TabChrome::load();
        let prefs = Prefs::entity(cx);
        let (sidebar_visible, sidebar_width) = Self::sidebar_state(&prefs, cx);
        cx.observe(&prefs, |this, prefs, cx| {
            if this.chrome.resize.is_none() {
                (this.sidebar_visible, this.sidebar_width) = Self::sidebar_state(&prefs, cx);
            }
            cx.notify();
        })
        .detach();
        Workspace { tabs: Vec::new(), selected: 0, sidebar_visible, sidebar_width, chrome, overlays: None }
    }

    fn sidebar_state(prefs: &Entity<Prefs>, cx: &App) -> (bool, f32) {
        let prefs = prefs.read(cx).preferences();
        (prefs.tab_sidebar_visible(), prefs.tab_sidebar_width() as f32)
    }

    /// Asks before closing the window while a tab runs a command.
    pub(crate) fn install_window_hooks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.chrome.hover_card.watch_keys(window, cx);
        cx.observe_window_bounds(window, |this, _, cx| this.chrome.hover_card.dismiss(cx)).detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if !window.is_window_active() { this.chrome.hover_card.dismiss(cx); }
        }).detach();
        let workspace = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(workspace) = workspace.upgrade() else { return true };
            if workspace.read(cx).chrome.closing {
                return true;
            }
            workspace.update(cx, |workspace, cx| workspace.request_close_window(window, cx));
            false
        });
    }

    /// Keeps the window title at "<title> [<branch>]".
    fn sync_window_title(&mut self, window: &mut Window, cx: &App) {
        let tab = &self.tabs[self.selected];
        let branch = self.chrome.git.status_for(tab.id).map(|status| status.titlebar_branch());
        let title = window_title(&self.tab_title(tab, cx), branch.as_deref());
        if title != self.chrome.window_title {
            window.set_window_title(&title);
            self.chrome.window_title = title;
        }
        self.chrome.window_bounds = window.bounds();
    }

    pub fn selected_view(&self) -> &Entity<TerminalView> {
        &self.tabs[self.selected].view
    }

    pub fn add_tab(&mut self, cwd: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let inherited = self.inherited_font_size(cx);
        let (view, splits) = Splits::start(cwd, window, cx);
        if let Some(size) = inherited {
            view.update(cx, |view, cx| view.set_font_size(size, cx));
        }
        let (at, group) = if self.tabs.is_empty() { (0, None) } else { self.new_tab_placement(cx) };
        let id = self.next_tab_id();
        let tab = Tab { id, view, splits, color: TerminalTabColor::None, title_override: None, group };
        self.tabs.insert(at, tab);
        self.select(at, window, cx);
        self.tabs_changed(cx);
    }

    /// The font size a new tab takes from the selected one, unless `window-inherit-font-size` is off.
    fn inherited_font_size(&self, cx: &App) -> Option<f32> {
        let enabled = cx.global::<Theme>().config.get("window-inherit-font-size").is_none_or(|value| value != "false");
        self.tabs.get(self.selected).filter(|_| enabled).map(|tab| tab.view.read(cx).font_size)
    }

    pub fn select(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.chrome.hover_card.dismiss(cx);
        self.selected = index;
        for pane in self.tabs[index].splits.panes() {
            pane.update(cx, |pane, _| pane.clear_bell());
        }
        let focus = self.tabs[index].view.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
        cx.notify();
    }

    pub fn close_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.chrome.hover_card.dismiss(cx);
        if self.tabs.len() == 1 {
            self.close_window(window, cx);
            return;
        }
        self.tabs.remove(index);
        self.selected = self.selected.min(self.tabs.len() - 1);
        if index < self.selected {
            self.selected -= 1;
        }
        self.prune_groups();
        self.select(self.selected, window, cx);
        self.tabs_changed(cx);
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab_from_selected(window, cx);
    }

    fn close_selected(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        let id = self.tabs[self.selected].id;
        self.request_close_tab(id, window, cx);
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.step_tab(1, window, cx);
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        self.step_tab(-1, window, cx);
    }

    fn toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.set_sidebar_visible(!self.sidebar_visible, cx);
    }

    pub fn set_sidebar_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.chrome.hover_card.dismiss(cx);
        if Spec::of(cx).tabs_in_titlebar() {
            return;
        }
        self.sidebar_visible = visible;
        Prefs::update(cx, |prefs| prefs.set_tab_sidebar_visible(visible));
        cx.notify();
    }

    pub fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.sidebar_width = width.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        cx.notify();
    }

    /// Persists the sidebar width once a resize ends.
    pub fn end_sidebar_resize(&mut self, cx: &mut Context<Self>) {
        if self.chrome.resize.take().is_some() {
            SidebarDragging::set(false, cx);
            let width = f64::from(self.sidebar_width);
            Prefs::update(cx, |prefs| prefs.set_tab_sidebar_width(width));
            cx.notify();
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reset_zoom = splits::reset_zoom_button(self, cx);
        let overlays = self.overlays(window, cx);
        self.sync_window_title(window, cx);
        self.sync_window_chrome(window, cx);
        if overlays.read(cx).settings_is_open() {
            let workspace = cx.entity();
            let root = div().relative().size_full().key_context("Workspace");
            return panels::attach(root, &overlays, move |cx| workspace.read(cx).host_info(cx))
                .child(overlays)
                .into_any_element();
        }
        let theme = cx.global::<Theme>();
        let background = theme.background().opacity(theme.config.background_opacity() as f32);
        let spec = Spec::of(cx);
        let titlebar_height = spec.titlebar_height();
        let in_titlebar = spec.tabs_in_titlebar();
        let sidebar_shown = self.sidebar_visible && !in_titlebar;
        let bar_shown = !sidebar_shown && !in_titlebar && self.tabs.len() > 1;
        let title_strip = (in_titlebar && self.tabs.len() > 1).then(|| tab_bar::render(self, true, window, cx));

        let mut root = div()
            .relative()
            .size_full()
            .h_flex()
            .bg(background)
            .key_context("Workspace")
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_selected))
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::toggle_sidebar))
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| this.chrome.hover_card.dismiss(cx)))
            .on_drag_move(cx.listener(|this, event: &DragMoveEvent<ResizeDrag>, window, cx| {
                sidebar::resize_to(this, event.event.position.x, window, cx);
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.end_sidebar_resize(cx)));
        root = splits::register(root, cx);
        root = tabs::register(root, cx);
        let workspace = cx.entity();
        root = panels::attach(root, &overlays, move |cx| workspace.read(cx).host_info(cx));
        if sidebar_shown {
            root = root.child(sidebar::render(self, titlebar_height, window, cx));
        }
        root.child(
            div()
                .v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(div().h(px(titlebar_height)).flex_none())
                .when(bar_shown, |column| column.child(tab_bar::render(self, false, window, cx)))
                .child(div().flex_1().min_h_0().child(splits::render(self, cx))),
        )
        .when(titlebar_height > 0.0, |root| root.child(titlebar::render(self, &spec, sidebar_shown, title_strip, reset_zoom, window, cx)))
        .child(overlays)
        .into_any_element()
    }
}
