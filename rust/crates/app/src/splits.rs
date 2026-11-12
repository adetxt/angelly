//! Terminal splits: every tab owns a [`SplitTree`] of panes. This module holds
//! the tree operations behind the split actions, the recursive renderer with
//! its draggable dividers, and the default split keybinds.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use angelly_core::persist::split_tree::{
    Component, Direction, FocusDirection, NewDirection, Node, Path, SpatialDirection,
};
use angelly_core::persist::{Size as TreeSize, SplitTree};
use gpui_kit::*;
use terminal::Rgb;

use crate::keybinds::{AppAction, Direction as KeyDirection, Perform, SplitDirection, SplitFocus};
use crate::terminal_view::{TerminalEvent, TerminalView};
use crate::theme::{hsla_of, Theme};
use crate::workspace::Workspace;

actions!(
    splits,
    [
        SplitRight,
        SplitDown,
        SplitLeft,
        SplitUp,
        ClosePane,
        GotoSplitPrevious,
        GotoSplitNext,
        GotoSplitUp,
        GotoSplitDown,
        GotoSplitLeft,
        GotoSplitRight,
        EqualizeSplits,
        ResizeSplitUp,
        ResizeSplitDown,
        ResizeSplitLeft,
        ResizeSplitRight,
        ToggleSplitZoom,
    ]
);

/// Pixels moved by the default `resize_split` keybinds.
const RESIZE_STEP: u16 = 10;
/// Smallest pane a divider drag can leave, in points.
const MIN_PANE_SIZE: f32 = 10.0;
const DIVIDER_VISIBLE: f32 = 1.0;
const DIVIDER_HIT: f32 = 6.0;

type Pane = Entity<TerminalView>;

/// A tab's panes: the tree plus the subscriptions that keep the workspace
/// informed about each pane (shell exit, focus).
pub struct Splits {
    tree: SplitTree<Pane>,
    subscriptions: Vec<PaneSubscriptions>,
    area: Rc<Cell<gpui_kit::Size<Pixels>>>,
}

struct PaneSubscriptions {
    pane: Pane,
    _observe: Subscription,
    _focus: Subscription,
    _events: Subscription,
}

impl Splits {
    /// Starts a tree with a single new pane, returning it as the focused pane.
    pub fn start(cwd: Option<PathBuf>, window: &mut Window, cx: &mut Context<Workspace>) -> (Pane, Splits) {
        let pane = cx.new(|cx| TerminalView::new(cwd, cx));
        let subscriptions = vec![subscribe(&pane, window, cx)];
        let splits = Splits {
            tree: SplitTree::with_view(pane.clone()),
            subscriptions,
            area: Rc::default(),
        };
        (pane, splits)
    }

    pub fn contains(&self, pane: &Pane) -> bool {
        self.tree.find_by(|p| p == pane).is_some()
    }

    /// The panes of the tab, in tree order.
    pub fn panes(&self) -> Vec<Pane> {
        self.tree.views().into_iter().cloned().collect()
    }

    /// The tree with every pane replaced by `f(pane)`.
    pub fn map_panes<U: Clone + PartialEq>(&self, f: impl FnMut(&Pane) -> U) -> SplitTree<U> {
        self.tree.map(f)
    }

    /// Rebuilds a tab's panes from a saved tree: `cwd` gives each pane's starting
    /// directory and `is_focused` marks the pane that gets focus (the first by default).
    pub fn restore<S: Clone + PartialEq>(
        saved: &SplitTree<S>,
        cwd: impl Fn(&S) -> Option<PathBuf>,
        is_focused: impl Fn(&S) -> bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> (Pane, Splits) {
        let mut focused: Option<Pane> = None;
        let mut subscriptions = Vec::new();
        let tree = saved.map(|state| {
            let pane = cx.new(|cx| TerminalView::new(cwd(state), cx));
            subscriptions.push(subscribe(&pane, window, cx));
            if focused.is_none() || is_focused(state) {
                focused = Some(pane.clone());
            }
            pane
        });
        let focused = focused.expect("a saved tree has a pane");
        (focused, Splits { tree, subscriptions, area: Rc::default() })
    }

    /// The panes of a tab that another workspace owned, now watched by this one.
    pub fn adopt(from: Splits, window: &mut Window, cx: &mut Context<Workspace>) -> Splits {
        let subscriptions = from.tree.views().into_iter().map(|pane| subscribe(pane, window, cx)).collect();
        Splits { tree: from.tree, subscriptions, area: Rc::default() }
    }

    pub fn is_zoomed(&self) -> bool {
        self.tree.is_zoomed()
    }

    fn area(&self) -> TreeSize {
        let size = self.area.get();
        TreeSize::new(f64::from(f32::from(size.width)), f64::from(f32::from(size.height)))
    }
}

fn subscribe(pane: &Pane, window: &mut Window, cx: &mut Context<Workspace>) -> PaneSubscriptions {
    let observe = cx.observe_in(pane, window, |this, pane, window, cx| {
        if pane.read(cx).should_close() {
            this.close_pane(&pane, window, cx);
        }
        cx.notify();
    });
    let handle = pane.read(cx).focus_handle().clone();
    let focused = pane.downgrade();
    let focus = cx.on_focus_in(&handle, window, move |this, _, cx| {
        if let Some(pane) = focused.upgrade() {
            this.pane_focused(&pane, cx);
        }
    });
    let events = cx.subscribe_in(pane, window, |this, _, event: &TerminalEvent, window, cx| {
        if matches!(event, TerminalEvent::Pwd(_)) {
            this.refresh_git(false, cx);
        }
        if let TerminalEvent::Clipboard(prompt) = event {
            if let Some(request) = prompt.take() {
                this.show_clipboard_request(request, window, cx);
            }
        }
    });
    PaneSubscriptions { pane: pane.clone(), _observe: observe, _focus: focus, _events: events }
}

fn leaf<V: Clone>(view: &V) -> Node<V> {
    Node::Leaf(view.clone())
}

/// The pane that takes focus when `view` closes: the next one, or the previous
/// one when `view` is the first.
fn focus_after_closing<V: Clone + PartialEq>(tree: &SplitTree<V>, view: &V) -> Option<V> {
    let direction = if tree.root()?.leftmost_leaf() == view {
        FocusDirection::Next
    } else {
        FocusDirection::Previous
    };
    tree.focus_target(direction, &leaf(view))
}

/// The tree with `view` zoomed, or unzoomed when it already is. `None` when
/// there is nothing to zoom into.
fn toggled_zoom<V: Clone + PartialEq>(tree: &SplitTree<V>, view: &V) -> Option<SplitTree<V>> {
    let node = leaf(view);
    if tree.zoomed() == Some(&node) {
        return Some(SplitTree::new(tree.root().cloned(), None));
    }
    tree.is_split()
        .then(|| SplitTree::new(tree.root().cloned(), Some(node)))
}

/// Where `direction` leads from `view`, and the tree to show once there. A zoomed
/// tree follows the focus when `preserve_zoom`, otherwise it unzooms.
fn navigated<V: Clone + PartialEq>(
    tree: &SplitTree<V>,
    view: &V,
    direction: FocusDirection,
    preserve_zoom: bool,
) -> Option<(SplitTree<V>, V)> {
    let target = tree.focus_target(direction, &leaf(view))?;
    let tree = match tree.zoomed() {
        Some(_) => {
            let zoomed = preserve_zoom.then(|| leaf(&target));
            SplitTree::new(tree.root().cloned(), zoomed)
        }
        None => tree.clone(),
    };
    Some((tree, target))
}

/// Split ratio for a divider dragged to `offset` within a split spanning `extent`.
fn divider_ratio(offset: f32, extent: f32) -> f64 {
    if extent <= 2.0 * MIN_PANE_SIZE {
        return 0.5;
    }
    f64::from(offset.max(MIN_PANE_SIZE).min(extent - MIN_PANE_SIZE) / extent)
}

/// The fallback divider color: the background darkened by 40% (8% on light themes).
fn darkened(color: Rgb, dark: bool) -> Rgb {
    let scale = if dark { 0.6 } else { 0.92 };
    let channel = |c: u8| (f32::from(c) * scale).round() as u8;
    Rgb::new(channel(color.r), channel(color.g), channel(color.b))
}

fn path_key(path: &[Component]) -> String {
    path.iter()
        .map(|c| if *c == Component::Left { 'L' } else { 'R' })
        .collect()
}

impl Workspace {
    fn tree_of_selected(&self) -> &SplitTree<Pane> {
        &self.tabs[self.selected].splits.tree
    }

    pub(crate) fn pane_focused(&mut self, pane: &Pane, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.splits.contains(pane)) {
            tab.view = pane.clone();
            cx.notify();
        }
    }

    fn focus_pane(&mut self, pane: &Pane, window: &mut Window, cx: &mut Context<Self>) {
        let focus = pane.read(cx).focus_handle().clone();
        self.pane_focused(pane, cx);
        window.focus(&focus, cx);
    }

    pub fn split_focused(&mut self, direction: NewDirection, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.tabs[self.selected].view.clone();
        let cwd = focused.read(cx).cwd();
        let size = focused.read(cx).font_size;
        let pane = cx.new(|cx| TerminalView::new(cwd, cx));
        pane.update(cx, |pane, cx| pane.set_font_size(size, cx));
        let Ok(tree) = self.tree_of_selected().inserting(pane.clone(), &focused, direction) else {
            return;
        };
        let subscriptions = subscribe(&pane, window, cx);
        let splits = &mut self.tabs[self.selected].splits;
        splits.tree = tree;
        splits.subscriptions.push(subscriptions);
        self.focus_pane(&pane, window, cx);
    }

    /// Closes `pane`; the last pane of a tab closes the tab.
    pub fn close_pane(&mut self, pane: &Pane, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t.splits.contains(pane)) else {
            return;
        };
        let tab = &mut self.tabs[index];
        if tab.splits.tree.len() == 1 {
            self.close_tab(index, window, cx);
            return;
        }
        let next = (tab.view == *pane)
            .then(|| focus_after_closing(&tab.splits.tree, pane))
            .flatten();
        tab.splits.tree = tab.splits.tree.removing(&leaf(pane));
        tab.splits.subscriptions.retain(|s| s.pane != *pane);
        if let Some(next) = next {
            self.focus_pane(&next, window, cx);
        }
        cx.notify();
    }

    pub fn goto_split(&mut self, direction: FocusDirection, window: &mut Window, cx: &mut Context<Self>) {
        let preserve_zoom = cx
            .global::<Theme>()
            .config
            .get("split-preserve-zoom")
            .is_some_and(|v| v.split(',').any(|flag| flag.trim() == "navigation"));
        let tab = &mut self.tabs[self.selected];
        let Some((tree, target)) = navigated(&tab.splits.tree, &tab.view, direction, preserve_zoom) else {
            return;
        };
        tab.splits.tree = tree;
        self.focus_pane(&target, window, cx);
    }

    /// Moves `moved` beside `target` after a pane drag, keeping `moved` focused.
    fn move_pane(&mut self, moved: &Pane, target: &Pane, edge: NewDirection, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.splits.contains(moved) && tab.splits.contains(target)) else {
            return;
        };
        let Some(tree) = rearranged(&tab.splits.tree, moved, target, edge) else { return };
        tab.splits.tree = tree;
        self.focus_pane(moved, window, cx);
        self.tabs_changed(cx);
    }

    pub fn equalize_splits(&mut self, cx: &mut Context<Self>) {
        let splits = &mut self.tabs[self.selected].splits;
        splits.tree = splits.tree.equalized();
        cx.notify();
    }

    pub fn resize_split(&mut self, direction: SpatialDirection, pixels: u16, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.selected];
        let area = tab.splits.area();
        if let Ok(tree) = tab.splits.tree.resizing(&leaf(&tab.view), pixels, direction, area) {
            tab.splits.tree = tree;
            cx.notify();
        }
    }

    pub fn toggle_split_zoom(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = &mut self.tabs[self.selected];
        let Some(tree) = toggled_zoom(&tab.splits.tree, &tab.view) else {
            return;
        };
        tab.splits.tree = tree;
        let focused = tab.view.clone();
        self.focus_pane(&focused, window, cx);
    }

    fn set_split_ratio(&mut self, path: &Path, ratio: f64, cx: &mut Context<Self>) {
        let splits = &mut self.tabs[self.selected].splits;
        let Some(node) = splits.tree.root().and_then(|root| root.node_at(path)).cloned() else {
            return;
        };
        if let Ok(tree) = splits.tree.replacing(&node, node.resizing(ratio)) {
            splits.tree = tree;
            cx.notify();
        }
    }
}

/// Test hook behind `ANGELLY_SELFTEST_SPLITS`: builds a 2x2 grid in the first tab,
/// then applies the operation named by the value (`zoom`, `resize`, `close`).
pub fn selftest(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let Ok(mode) = std::env::var("ANGELLY_SELFTEST_SPLITS") else {
        return;
    };
    workspace.split_focused(NewDirection::Right, window, cx);
    workspace.split_focused(NewDirection::Down, window, cx);
    workspace.goto_split(FocusDirection::Spatial(SpatialDirection::Left), window, cx);
    workspace.split_focused(NewDirection::Down, window, cx);
    match mode.as_str() {
        "zoom" => workspace.toggle_split_zoom(window, cx),
        "resize" => cx
            .spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let _ = this.update(cx, |workspace, cx| workspace.resize_split(SpatialDirection::Right, 120, cx));
            })
            .detach(),
        "close" => {
            let focused = workspace.tabs[workspace.selected].view.clone();
            workspace.close_pane(&focused, window, cx);
        }
        _ => {}
    }
}

/// Registers the split action handlers on the workspace root.
pub fn register(root: Div, cx: &mut Context<Workspace>) -> Div {
    root.on_action(cx.listener(|this, _: &SplitRight, w, cx| this.split_focused(NewDirection::Right, w, cx)))
        .on_action(cx.listener(|this, _: &SplitDown, w, cx| this.split_focused(NewDirection::Down, w, cx)))
        .on_action(cx.listener(|this, _: &SplitLeft, w, cx| this.split_focused(NewDirection::Left, w, cx)))
        .on_action(cx.listener(|this, _: &SplitUp, w, cx| this.split_focused(NewDirection::Up, w, cx)))
        .on_action(cx.listener(|this, _: &ClosePane, w, cx| {
            let focused = this.tabs[this.selected].view.clone();
            this.close_pane(&focused, w, cx);
        }))
        .on_action(cx.listener(|this, _: &GotoSplitPrevious, w, cx| this.goto_split(FocusDirection::Previous, w, cx)))
        .on_action(cx.listener(|this, _: &GotoSplitNext, w, cx| this.goto_split(FocusDirection::Next, w, cx)))
        .on_action(cx.listener(|this, _: &GotoSplitUp, w, cx| {
            this.goto_split(FocusDirection::Spatial(SpatialDirection::Up), w, cx)
        }))
        .on_action(cx.listener(|this, _: &GotoSplitDown, w, cx| {
            this.goto_split(FocusDirection::Spatial(SpatialDirection::Down), w, cx)
        }))
        .on_action(cx.listener(|this, _: &GotoSplitLeft, w, cx| {
            this.goto_split(FocusDirection::Spatial(SpatialDirection::Left), w, cx)
        }))
        .on_action(cx.listener(|this, _: &GotoSplitRight, w, cx| {
            this.goto_split(FocusDirection::Spatial(SpatialDirection::Right), w, cx)
        }))
        .on_action(cx.listener(|this, _: &EqualizeSplits, _, cx| this.equalize_splits(cx)))
        .on_action(cx.listener(|this, _: &ResizeSplitUp, _, cx| this.resize_split(SpatialDirection::Up, RESIZE_STEP, cx)))
        .on_action(cx.listener(|this, _: &ResizeSplitDown, _, cx| {
            this.resize_split(SpatialDirection::Down, RESIZE_STEP, cx)
        }))
        .on_action(cx.listener(|this, _: &ResizeSplitLeft, _, cx| {
            this.resize_split(SpatialDirection::Left, RESIZE_STEP, cx)
        }))
        .on_action(cx.listener(|this, _: &ResizeSplitRight, _, cx| {
            this.resize_split(SpatialDirection::Right, RESIZE_STEP, cx)
        }))
        .on_action(cx.listener(|this, _: &ToggleSplitZoom, w, cx| this.toggle_split_zoom(w, cx)))
        .on_action(cx.listener(|this, Perform(action): &Perform, window, cx| match action {
            AppAction::NewSplit(direction) => this.split_focused(new_direction(*direction), window, cx),
            AppAction::GotoSplit(focus) => this.goto_split(focus_direction(*focus), window, cx),
            AppAction::ResizeSplit(direction, pixels) => this.resize_split(spatial(*direction), *pixels, cx),
            AppAction::EqualizeSplits => this.equalize_splits(cx),
            AppAction::ToggleSplitZoom => this.toggle_split_zoom(window, cx),
            _ => cx.propagate(),
        }))
}

fn new_direction(direction: SplitDirection) -> NewDirection {
    match direction {
        SplitDirection::Right | SplitDirection::Auto => NewDirection::Right,
        SplitDirection::Down => NewDirection::Down,
        SplitDirection::Left => NewDirection::Left,
        SplitDirection::Up => NewDirection::Up,
    }
}

fn spatial(direction: KeyDirection) -> SpatialDirection {
    match direction {
        KeyDirection::Up => SpatialDirection::Up,
        KeyDirection::Down => SpatialDirection::Down,
        KeyDirection::Left => SpatialDirection::Left,
        KeyDirection::Right => SpatialDirection::Right,
    }
}

fn focus_direction(focus: SplitFocus) -> FocusDirection {
    match focus {
        SplitFocus::Previous => FocusDirection::Previous,
        SplitFocus::Next => FocusDirection::Next,
        SplitFocus::Up => FocusDirection::Spatial(SpatialDirection::Up),
        SplitFocus::Down => FocusDirection::Spatial(SpatialDirection::Down),
        SplitFocus::Left => FocusDirection::Spatial(SpatialDirection::Left),
        SplitFocus::Right => FocusDirection::Spatial(SpatialDirection::Right),
        SplitFocus::Top => FocusDirection::Spatial(SpatialDirection::Up),
        SplitFocus::Bottom => FocusDirection::Spatial(SpatialDirection::Down),
    }
}

/// The titlebar button that leaves a zoomed split, when the tab is zoomed.
pub fn reset_zoom_button(workspace: &Workspace, cx: &mut Context<Workspace>) -> Option<impl IntoElement> {
    let tab = &workspace.tabs[workspace.selected];
    if !tab.splits.is_zoomed() {
        return None;
    }
    let accent = cx.global::<Theme>().accent;
    Some(
        div()
            .id("reset-zoom")
            .role(Role::Button)
            .aria_label("Reset Zoom")
            .size(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_color(accent)
            .cursor_pointer()
            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Reset Zoom").build(window, cx))
            .on_click(cx.listener(|this, _, window, cx| this.toggle_split_zoom(window, cx)))
            .child(svg().path("ui/reset-zoom.svg").size(px(14.)).text_color(accent)),
    )
}

struct DividerDrag {
    path: Vec<Component>,
}

/// A pane being carried by its grab handle.
#[derive(Clone)]
struct PaneDrag {
    pane: Pane,
}

struct PaneDragPreview;

impl Render for PaneDragPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_0()
    }
}

/// Grab handle size, and the share of the pane's height whose hover reveals it.
const HANDLE_WIDTH: f32 = 80.0;
const HANDLE_HEIGHT: f32 = 12.0;
const HANDLE_REGION: f32 = 0.2;

/// The edge of a pane nearest to a point, which decides where a dropped pane lands. The
/// pane is cut along its diagonals into four triangles.
fn drop_edge(x: f32, y: f32, width: f32, height: f32) -> NewDirection {
    let (rx, ry) = (x / width.max(1.0), y / height.max(1.0));
    let distances = [
        (rx, NewDirection::Left),
        (1.0 - rx, NewDirection::Right),
        (ry, NewDirection::Up),
        (1.0 - ry, NewDirection::Down),
    ];
    distances.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map_or(NewDirection::Right, |(_, edge)| edge)
}

/// `tree` with `moved` taken out and placed beside `target` on the side `edge`. `None` when
/// the pane would land on itself or the tree has nothing to rearrange.
fn rearranged<V: Clone + PartialEq>(tree: &SplitTree<V>, moved: &V, target: &V, edge: NewDirection) -> Option<SplitTree<V>> {
    if moved == target || !tree.is_split() {
        return None;
    }
    tree.removing(&leaf(moved)).inserting(moved.clone(), target, edge).ok()
}

struct Style {
    divider: Hsla,
    unfocused: Option<Hsla>,
    focused: Pane,
    /// Panes can be rearranged: the tab is split and not zoomed.
    rearrangeable: bool,
    accent: Hsla,
    handle: Hsla,
    /// Where a dragged pane would land, while one is dragged.
    drop: Option<(EntityId, NewDirection)>,
}

/// The split area of the selected tab: the zoomed pane, or the whole tree.
pub fn render(workspace: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let tab = &workspace.tabs[workspace.selected];
    let tree = &tab.splits.tree;
    let Some(node) = tree.zoomed().or(tree.root()) else {
        return div().size_full();
    };
    let theme = cx.global::<Theme>();
    let background = theme.colors.background;
    let to_rgb = |c: angelly_core::config::Rgb| Rgb::new(c.r, c.g, c.b);
    let divider = theme
        .config
        .split_divider_color()
        .map_or_else(|| darkened(background, theme.dark), to_rgb);
    let fill = theme.config.unfocused_split_fill().map_or(background, to_rgb);
    let dim = 1.0 - theme.config.unfocused_split_opacity() as f32;
    let style = Style {
        divider: hsla_of(divider),
        unfocused: (tree.is_split() && dim > 0.0).then(|| hsla_of(fill).opacity(dim)),
        focused: tab.view.clone(),
        rearrangeable: tree.is_split() && !tree.is_zoomed(),
        accent: theme.accent,
        handle: theme.primary.opacity(0.18),
        drop: workspace.chrome.pane_drop.filter(|_| cx.has_active_drag()),
    };
    let path = tree
        .root()
        .and_then(|root| root.path_to(node))
        .map(|p| p.path)
        .unwrap_or_default();
    let area = tab.splits.area.clone();
    div()
        .relative()
        .size_full()
        .child(
            canvas(move |bounds, _, _| area.set(bounds.size), |_, _, _, _| {})
                .absolute()
                .inset_0(),
        )
        .child(subtree(node, path, &style, cx))
}

/// `ANGELLY_SELFTEST_PANE_DRAG` shows every grab handle and a drop zone on the focused pane.
fn pane_drag_selftest() -> bool {
    std::env::var_os("ANGELLY_SELFTEST_PANE_DRAG").is_some()
}

/// A pane with its dimming, grab handle and drop zones.
fn leaf_element(pane: &Pane, style: &Style, cx: &mut Context<Workspace>) -> AnyElement {
    let id = pane.entity_id();
    let group = SharedString::from(format!("pane-{}", id.as_u64()));
    let mut element = div().relative().size_full().child(pane.clone());
    if let Some(fill) = style.unfocused.filter(|_| *pane != style.focused) {
        element = element.child(div().absolute().inset_0().bg(fill));
    }
    if !style.rearrangeable {
        return element.into_any_element();
    }
    let handle = div()
        .id(("pane-handle", id.as_u64()))
        .role(Role::Button)
        .aria_label("Move Pane")
        .w(px(HANDLE_WIDTH))
        .h(px(HANDLE_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .rounded_b(px(6.))
        .bg(style.handle)
        .cursor_grab()
        .opacity(if pane_drag_selftest() { 1. } else { 0. })
        .group_hover(group.clone(), |s| s.opacity(1.))
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, _| this.chrome.pane_drop = None))
        .on_drag(PaneDrag { pane: pane.clone() }, |_, _, _, cx| cx.new(|_| PaneDragPreview))
        .child(svg().path("ui/ellipsis.svg").size(px(10.)).text_color(gpui_kit::white().opacity(0.8)));
    element = element.child(
        div()
            .id(("pane-handle-region", id.as_u64()))
            .group(group)
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(relative(HANDLE_REGION))
            .flex()
            .justify_center()
            .child(handle),
    );
    if cx.has_active_drag() || pane_drag_selftest() {
        let target = pane.clone();
        let zone = style.drop.filter(|(over, _)| *over == id).map(|(_, edge)| edge).or_else(|| {
            (pane_drag_selftest() && *pane == style.focused).then_some(NewDirection::Right)
        });
        element = element.child(
            div()
                .id(("pane-drop", id.as_u64()))
                .absolute()
                .inset_0()
                .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<PaneDrag>, _, cx| {
                    let (bounds, position) = (event.bounds, event.event.position);
                    if !bounds.contains(&position) {
                        if this.chrome.pane_drop.is_some_and(|(over, _)| over == id) {
                            this.chrome.pane_drop = None;
                            cx.notify();
                        }
                        return;
                    }
                    let edge = drop_edge(
                        f32::from(position.x - bounds.origin.x),
                        f32::from(position.y - bounds.origin.y),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    );
                    if this.chrome.pane_drop != Some((id, edge)) {
                        this.chrome.pane_drop = Some((id, edge));
                        cx.notify();
                    }
                }))
                .on_drop(cx.listener(move |this, drag: &PaneDrag, window, cx| {
                    if let Some((_, edge)) = this.chrome.pane_drop.take() {
                        this.move_pane(&drag.pane, &target, edge, window, cx);
                    }
                }))
                .children(zone.map(|edge| {
                    let half = div().absolute().bg(style.accent.opacity(0.3));
                    match edge {
                        NewDirection::Left => half.top_0().bottom_0().left_0().w(relative(0.5)),
                        NewDirection::Right => half.top_0().bottom_0().right_0().w(relative(0.5)),
                        NewDirection::Up => half.left_0().right_0().top_0().h(relative(0.5)),
                        NewDirection::Down => half.left_0().right_0().bottom_0().h(relative(0.5)),
                    }
                })),
        );
    }
    element.into_any_element()
}

fn subtree(node: &Node<Pane>, path: Vec<Component>, style: &Style, cx: &mut Context<Workspace>) -> AnyElement {
    match node {
        Node::Leaf(pane) => leaf_element(pane, style, cx),
        Node::Split(split) => {
            let ratio = split.ratio as f32;
            let horizontal = split.direction == Direction::Horizontal;
            let child_path = |component| {
                let mut p = path.clone();
                p.push(component);
                p
            };
            let first = subtree(&split.left, child_path(Component::Left), style, cx);
            let second = subtree(&split.right, child_path(Component::Right), style, cx);
            let key = path_key(&path);
            let drag_path = path.clone();

            let (first_pane, second_pane) = if horizontal {
                (
                    div().absolute().top_0().bottom_0().left_0().w(relative(ratio)),
                    div().absolute().top_0().bottom_0().right_0().left(relative(ratio)).ml(px(DIVIDER_VISIBLE)),
                )
            } else {
                (
                    div().absolute().left_0().right_0().top_0().h(relative(ratio)),
                    div().absolute().left_0().right_0().bottom_0().top(relative(ratio)).mt(px(DIVIDER_VISIBLE)),
                )
            };
            let inset = DIVIDER_HIT / 2.0;
            let line = div().absolute().bg(style.divider);
            let divider = if horizontal {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(ratio))
                    .ml(px(-inset))
                    .w(px(DIVIDER_VISIBLE + DIVIDER_HIT))
                    .cursor_col_resize()
                    .child(line.top_0().bottom_0().left(px(inset)).w(px(DIVIDER_VISIBLE)))
            } else {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top(relative(ratio))
                    .mt(px(-inset))
                    .h(px(DIVIDER_VISIBLE + DIVIDER_HIT))
                    .cursor_row_resize()
                    .child(line.left_0().right_0().top(px(inset)).h(px(DIVIDER_VISIBLE)))
            };

            div()
                .relative()
                .size_full()
                .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<DividerDrag>, _, cx| {
                    if event.drag(cx).path != drag_path {
                        return;
                    }
                    let (offset, extent) = if horizontal {
                        (event.event.position.x - event.bounds.origin.x, event.bounds.size.width)
                    } else {
                        (event.event.position.y - event.bounds.origin.y, event.bounds.size.height)
                    };
                    let ratio = divider_ratio(f32::from(offset), f32::from(extent));
                    this.set_split_ratio(&Path { path: drag_path.clone() }, ratio, cx);
                }))
                .child(first_pane.child(first))
                .child(second_pane.child(second))
                .child(
                    divider
                        .occlude()
                        .id(SharedString::from(format!("divider-{key}")))
                        .on_drag(DividerDrag { path: path.clone() }, |_, _, _, cx| cx.new(|_| Empty))
                        .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                            if event.click_count() == 2 {
                                this.equalize_splits(cx);
                            }
                        })),
                )
                .into_any_element()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    fn pair() -> SplitTree<u32> {
        SplitTree::with_view(1).inserting(2, &1, NewDirection::Right).unwrap()
    }

    fn grid() -> SplitTree<u32> {
        pair()
            .inserting(3, &1, NewDirection::Down)
            .unwrap()
            .inserting(4, &2, NewDirection::Down)
            .unwrap()
    }

    #[test]
    fn divider_ratio_follows_the_pointer() {
        assert_eq!(divider_ratio(300.0, 600.0), 0.5);
        assert_eq!(divider_ratio(150.0, 600.0), 0.25);
    }

    #[test]
    fn divider_ratio_keeps_the_minimum_pane_size() {
        assert_eq!(divider_ratio(-50.0, 600.0), f64::from(10.0f32 / 600.0));
        assert_eq!(divider_ratio(900.0, 600.0), f64::from(590.0f32 / 600.0));
        assert_eq!(divider_ratio(5.0, 15.0), 0.5);
    }

    #[test]
    fn dark_backgrounds_darken_by_forty_percent() {
        assert_eq!(darkened(Rgb::new(100, 200, 50), true), Rgb::new(60, 120, 30));
        assert_eq!(darkened(Rgb::new(100, 200, 50), false), Rgb::new(92, 184, 46));
    }

    #[test]
    fn closing_the_first_pane_focuses_the_next() {
        assert_eq!(focus_after_closing(&grid(), &3), Some(1));
        assert_eq!(focus_after_closing(&pair(), &1), Some(2));
    }

    #[test]
    fn closing_a_later_pane_focuses_the_previous() {
        assert_eq!(focus_after_closing(&pair(), &2), Some(1));
        assert_eq!(focus_after_closing(&grid(), &4), Some(2));
    }

    #[test]
    fn zoom_toggles_on_and_off() {
        let zoomed = toggled_zoom(&pair(), &2).unwrap();
        assert!(zoomed.is_zoomed());
        assert_eq!(zoomed.zoomed(), Some(&Node::Leaf(2)));
        assert!(!toggled_zoom(&zoomed, &2).unwrap().is_zoomed());
    }

    #[test]
    fn zoom_moves_to_another_pane() {
        let zoomed = toggled_zoom(&pair(), &1).unwrap();
        assert_eq!(toggled_zoom(&zoomed, &2).unwrap().zoomed(), Some(&Node::Leaf(2)));
    }

    #[test]
    fn a_lone_pane_cannot_zoom() {
        assert!(toggled_zoom(&SplitTree::with_view(1), &1).is_none());
    }

    #[test]
    fn navigation_follows_the_layout() {
        let right = FocusDirection::Spatial(SpatialDirection::Right);
        let down = FocusDirection::Spatial(SpatialDirection::Down);
        assert_eq!(navigated(&grid(), &1, right, false).unwrap().1, 2);
        assert_eq!(navigated(&grid(), &1, down, false).unwrap().1, 3);
        assert_eq!(navigated(&grid(), &2, down, false).unwrap().1, 4);
        assert!(navigated(&grid(), &3, down, false).is_none());
    }

    #[test]
    fn navigation_unzooms_unless_preserved() {
        let next = FocusDirection::Next;
        let zoomed = toggled_zoom(&pair(), &1).unwrap();
        let (tree, target) = navigated(&zoomed, &1, next, false).unwrap();
        assert_eq!(target, 2);
        assert!(!tree.is_zoomed());
        let (tree, target) = navigated(&zoomed, &1, next, true).unwrap();
        assert_eq!(target, 2);
        assert_eq!(tree.zoomed(), Some(&Node::Leaf(2)));
    }

    #[test]
    fn the_nearest_edge_decides_the_drop() {
        assert_eq!(drop_edge(5.0, 50.0, 100.0, 100.0), NewDirection::Left);
        assert_eq!(drop_edge(95.0, 50.0, 100.0, 100.0), NewDirection::Right);
        assert_eq!(drop_edge(50.0, 4.0, 100.0, 100.0), NewDirection::Up);
        assert_eq!(drop_edge(50.0, 96.0, 100.0, 100.0), NewDirection::Down);
        assert_eq!(drop_edge(20.0, 10.0, 100.0, 100.0), NewDirection::Up);
        assert_eq!(drop_edge(10.0, 20.0, 100.0, 100.0), NewDirection::Left);
    }

    #[test]
    fn a_dropped_pane_lands_beside_the_target() {
        let tree = rearranged(&grid(), &3, &2, NewDirection::Right).unwrap();
        assert_eq!(tree.len(), 4);
        let right_of_two = navigated(&tree, &2, FocusDirection::Spatial(SpatialDirection::Right), false).unwrap().1;
        assert_eq!(right_of_two, 3);
    }

    #[test]
    fn a_pane_cannot_be_dropped_on_itself_or_alone() {
        assert!(rearranged(&grid(), &1, &1, NewDirection::Left).is_none());
        assert!(rearranged(&SplitTree::with_view(1), &1, &1, NewDirection::Left).is_none());
    }

    #[test]
    fn path_keys_spell_the_route() {
        assert_eq!(path_key(&[]), "");
        assert_eq!(path_key(&[Component::Left, Component::Right]), "LR");
    }
}
