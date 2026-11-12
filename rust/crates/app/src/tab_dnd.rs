//! Drag and drop of tabs and groups shared by the sidebar and the horizontal tab bar.

use gpui_kit::*;

use crate::chrome::{DragPreview, Dragged, DropHover, DropTarget};
use crate::tab_ops::{self, Drag, Edge, Feedback, Over};
use crate::theme::Theme;
use crate::workspace::Workspace;

/// Pointer positions are compared in steps so a drag does not repaint on every pixel.
const POSITION_STEPS: f32 = 8.0;

impl Workspace {
    fn drag_ops(&self, dragged: Dragged, target: DropTarget, position: f32) -> Option<(Drag, Over)> {
        let drag = match dragged {
            Dragged::Tab(id) => Drag::Tab(self.tab_index(id)?),
            Dragged::Group { anchor, .. } => Drag::Group(self.tab_index(anchor)?),
        };
        let over = match target {
            DropTarget::Tab(id) => Over::Tab(self.tab_index(id)?),
            DropTarget::Group { group, anchor, collapsed } => Over::GroupHeader {
                anchor: self.tab_index(anchor)?,
                group,
                landing: tab_ops::group_landing(collapsed, position),
            },
            DropTarget::End => Over::End,
        };
        Some((drag, over))
    }

    /// Tracks where an active drag hovers. Called for every candidate target on each move.
    pub fn drag_over(
        &mut self,
        target: DropTarget,
        dragged: Dragged,
        bounds: Bounds<Pixels>,
        pointer: Point<Pixels>,
        horizontal: bool,
        cx: &mut Context<Self>,
    ) {
        if bounds.contains(&pointer) {
            let (along, extent) = if horizontal {
                (pointer.x - bounds.origin.x, bounds.size.width)
            } else {
                (pointer.y - bounds.origin.y, bounds.size.height)
            };
            let fraction = (along / extent.max(px(1.))).clamp(0.0, 1.0);
            let position = (fraction * POSITION_STEPS).round() / POSITION_STEPS;
            let hover = DropHover { target, dragged, position };
            if self.chrome.drop != Some(hover) {
                self.chrome.drop = Some(hover);
                cx.notify();
            }
        } else if self.chrome.drop.is_some_and(|hover| hover.target == target) {
            self.chrome.drop = None;
            cx.notify();
        }
    }

    /// What the drag hovering `target` would do, for drawing the indicator.
    pub fn drop_feedback(&self, target: DropTarget, horizontal: bool, cx: &App) -> Feedback {
        let Some(hover) = self.chrome.active_drop(cx).filter(|hover| hover.target == target) else {
            return Feedback::None;
        };
        match self.drag_ops(hover.dragged, target, hover.position) {
            Some((drag, over)) => tab_ops::feedback(&self.tabs, drag, over, hover.position, horizontal),
            None => Feedback::None,
        }
    }

    pub fn drop_on(&mut self, target: DropTarget, dragged: Dragged, horizontal: bool, cx: &mut Context<Self>) {
        let position = self
            .chrome
            .drop
            .filter(|hover| hover.target == target)
            .map_or(0.5, |hover| hover.position);
        self.chrome.drop = None;
        let Some((drag, over)) = self.drag_ops(dragged, target, position) else { return };
        self.reorder(cx, |tabs| tab_ops::apply_drop(tabs, drag, over, position, horizontal));
    }
}

/// A 2pt accent bar on the leading/trailing (horizontal) or top/bottom (vertical) edge of a
/// drop target, inset 4pt on the cross axis.
pub fn indicator(edge: Edge, horizontal: bool, cx: &App) -> Div {
    let accent = cx.global::<Theme>().accent;
    let bar = div().absolute().rounded(px(1.)).bg(accent);
    match (horizontal, edge) {
        (true, Edge::Before) => bar.left_0().w(px(2.)).top(px(4.)).bottom(px(4.)),
        (true, Edge::After) => bar.right_0().w(px(2.)).top(px(4.)).bottom(px(4.)),
        (false, Edge::Before) => bar.top_0().h(px(2.)).left(px(4.)).right(px(4.)),
        (false, Edge::After) => bar.bottom_0().h(px(2.)).left(px(4.)).right(px(4.)),
    }
}

pub fn preview(title: &str, width: f32, cx: &mut App) -> Entity<DragPreview> {
    let title = title.to_string();
    cx.new(|_| DragPreview::new(title, width))
}

/// Registers the drop handling of one row on `element`.
pub fn drop_target<E: InteractiveElement + StatefulInteractiveElement>(
    element: E,
    target: DropTarget,
    horizontal: bool,
    cx: &mut Context<Workspace>,
) -> E {
    element
        .on_drag_move(cx.listener(move |this, event: &DragMoveEvent<Dragged>, _, cx| {
            let dragged = *event.drag(cx);
            this.drag_over(target, dragged, event.bounds, event.event.position, horizontal, cx);
        }))
        .on_drop(cx.listener(move |this, dragged: &Dragged, _, cx| this.drop_on(target, *dragged, horizontal, cx)))
}
