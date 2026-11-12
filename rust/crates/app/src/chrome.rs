//! State shared by the tab sidebar, the horizontal tab bar and the titlebar:
//! transient interaction state, drag and drop, and the row fade animations.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use angelly_core::persist::TabGroupStore;
use gpui_kit::*;
use uuid::Uuid;

use crate::git_status::GitStatuses;
use crate::open_with::OpenWith;
use crate::theme::Theme;
use crate::window_chrome::Spec;
use crate::workspace::TabId;

/// Duration of the hover and selection fades (`easeOut(0.12)` in the Swift app).
const FADE: Duration = Duration::from_millis(120);

/// Identifies a sidebar or tab bar row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowKey {
    Tab(TabId),
    /// A group header, anchored on its first tab so a split group has unique rows.
    Group(Uuid, TabId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FadeKind {
    Hover,
    Selected,
}

struct Fade {
    from: f32,
    to: f32,
    start: Instant,
}

impl Fade {
    fn value(&self, now: Instant) -> f32 {
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / FADE.as_secs_f32()).min(1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }

    fn done(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) >= FADE
    }
}

/// Animated 0..1 levels per row, retargeted whenever the wanted level changes.
#[derive(Default)]
pub struct Fades {
    levels: HashMap<(RowKey, FadeKind), Fade>,
}

impl Fades {
    /// The current level moving toward `target`. Asks for another frame while it moves.
    pub fn level(&mut self, key: RowKey, kind: FadeKind, target: f32, window: &mut Window) -> f32 {
        let now = Instant::now();
        let fade = self
            .levels
            .entry((key, kind))
            .or_insert(Fade { from: target, to: target, start: now - FADE });
        if fade.to != target {
            *fade = Fade { from: fade.value(now), to: target, start: now };
        }
        let value = fade.value(now);
        if !fade.done(now) {
            window.request_animation_frame();
        }
        value
    }

    /// Forgets rows that no longer exist.
    pub fn retain(&mut self, live: &[RowKey]) {
        self.levels.retain(|(key, _), _| live.contains(key));
    }
}

/// What is being dragged: a tab, or a group's run of tabs starting at `anchor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dragged {
    Tab(TabId),
    Group { id: Uuid, anchor: TabId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    Tab(TabId),
    /// A group header whose run starts at `anchor`.
    Group { group: Uuid, anchor: TabId, collapsed: bool },
    End,
}

/// Where a drag currently hovers, as a fraction (0 to 1) along the target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DropHover {
    pub target: DropTarget,
    pub dragged: Dragged,
    pub position: f32,
}

/// The image that follows the pointer while a tab or group is dragged.
pub struct DragPreview {
    title: SharedString,
    width: Pixels,
}

impl DragPreview {
    pub fn new(title: impl Into<SharedString>, width: f32) -> Self {
        Self { title: title.into(), width: px(width.min(200.0)) }
    }
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        div()
            .w(self.width)
            .h(px(32.))
            .px(px(8.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .bg(if theme.dark { rgb(0x1E1E1E).into() } else { white() })
            .text_size(px(12.))
            .text_color(theme.primary)
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .child(self.title.clone())
    }
}

/// An in-progress sidebar resize: the pointer and width when the drag started.
#[derive(Debug, Clone, Copy)]
pub struct Resize {
    pub pointer: f32,
    pub width: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ResizeDrag;

/// Whether a sidebar drag is in progress. Terminals keep their grid until it ends, so the shell
/// gets one resize instead of one per frame (it redraws its prompt for each, and the redraws
/// drift the prompt down whenever the width crosses the point where the prompt wraps).
#[derive(Debug, Clone, Copy, Default)]
pub struct SidebarDragging(pub bool);

impl Global for SidebarDragging {}

impl SidebarDragging {
    pub fn get(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|dragging| dragging.0)
    }

    pub fn set(dragging: bool, cx: &mut App) {
        cx.set_global(Self(dragging));
    }
}

pub struct TabChrome {
    pub groups: TabGroupStore,
    pub next_id: TabId,
    pub closing: bool,
    /// The group whose name is edited in place.
    pub renaming: Option<GroupRename>,
    pub hovered: Option<RowKey>,
    pub drop: Option<DropHover>,
    pub fades: Fades,
    pub resize: Option<Resize>,
    pub git: GitStatuses,
    pub hover_card: crate::tab_popover::HoverCard,
    pub tab_scroll: ScrollHandle,
    /// The row index the tab bar last scrolled to.
    pub scrolled_to: Option<usize>,
    pub window_title: String,
    pub window_bounds: Bounds<Pixels>,
    pub session_path: Option<PathBuf>,
    pub save_generation: u64,
    pub open_with: OpenWith,
    /// The window settings last applied, with the cell size they were applied for.
    pub window_spec: Option<(Spec, (f32, f32))>,
    /// The window opened without a saved frame, so `window-width` and `window-height` size it.
    pub sized_by_config: bool,
    /// Where the new-tab button was last painted, which its worktree popover hangs from.
    pub new_tab_bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The pane a dragged pane is over and the side it would land on.
    pub pane_drop: Option<(EntityId, angelly_core::persist::split_tree::NewDirection)>,
}

pub struct GroupRename {
    pub group: Uuid,
    pub input: Entity<gpui_kit::component::input::InputState>,
    pub _subscriptions: Vec<Subscription>,
}

impl TabChrome {
    /// Self tests have no data directory and save nothing.
    pub fn load() -> Self {
        let directory = crate::session::data_directory();
        TabChrome {
            groups: TabGroupStore::new(),
            next_id: 0,
            closing: false,
            renaming: None,
            hovered: None,
            drop: None,
            fades: Fades::default(),
            resize: None,
            git: GitStatuses::new(directory.is_some()),
            hover_card: crate::tab_popover::HoverCard::default(),
            tab_scroll: ScrollHandle::new(),
            scrolled_to: None,
            window_title: String::new(),
            window_bounds: Bounds::default(),
            session_path: crate::session::new_session_path(),
            save_generation: 0,
            open_with: OpenWith::default(),
            window_spec: None,
            sized_by_config: false,
            new_tab_bounds: Rc::default(),
            pane_drop: None,
        }
    }

    /// The hover shown while a drag is active; stale once the drag has ended.
    pub fn active_drop(&self, cx: &App) -> Option<DropHover> {
        self.drop.filter(|_| cx.has_active_drag())
    }
}
