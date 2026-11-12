//! The vertical tab sidebar: rows, group headers, the resize handle and drag and drop.

use angelly_core::persist::{GroupHeader, Row, TabItem};
use gpui_kit::component::input::Input;
use gpui_kit::component::Sizable as _;
use crate::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::StyledExt as _;
use gpui_kit::*;
use gpui_kit::base::ElementExt as _;

use crate::chrome::{Dragged, DropTarget, FadeKind, Resize, ResizeDrag, RowKey, SidebarDragging};
use crate::middle_text::MiddleText;
use crate::tab_colors::{display_color, SystemColors};
use crate::tab_dnd::{self, indicator};
use crate::tab_menus::{group_menu, tab_menu};
use crate::tab_ops::Feedback;
use crate::theme::Theme;
use crate::titlebar::new_tab_button;
use crate::workspace::{TabId, Workspace, SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, TITLEBAR_HEIGHT};

const ROW_HEIGHT: f32 = 32.0;
const HEADER_HEIGHT: f32 = 28.0;
const HANDLE_WIDTH: f32 = 6.0;

/// Colors the rows draw with, resolved once per render.
#[derive(Clone, Copy)]
pub struct Palette {
    pub primary: Hsla,
    pub secondary: Hsla,
    pub tertiary: Hsla,
    pub accent: Hsla,
    pub orange: Hsla,
    pub dark: bool,
}

impl Palette {
    pub fn of(theme: &Theme) -> Self {
        Palette {
            primary: theme.primary,
            secondary: theme.secondary(),
            tertiary: theme.tertiary(),
            accent: theme.accent,
            orange: SystemColors::orange(theme.dark),
            dark: theme.dark,
        }
    }

    pub fn tint(&self, alpha: f32) -> Hsla {
        self.primary.opacity(alpha)
    }
}

pub fn render(
    workspace: &mut Workspace,
    titlebar_height: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let palette = Palette::of(cx.global::<Theme>());
    let rows = workspace.rows(cx);
    let live: Vec<RowKey> = rows
        .iter()
        .map(|row| match row {
            Row::Tab(item) => RowKey::Tab(item.id),
            Row::Group { header, anchor } => RowKey::Group(header.id, *anchor),
        })
        .collect();
    workspace.chrome.fades.retain(&live);
    let dragging = cx.has_active_drag();

    let mut children = Vec::new();
    for row in &rows {
        children.push(match row {
            Row::Tab(item) => tab_row(workspace, item, palette, dragging, window, cx).into_any_element(),
            Row::Group { header, anchor } => {
                group_header(workspace, header, *anchor, palette, dragging, window, cx).into_any_element()
            }
        });
    }

    let end_feedback = workspace.drop_feedback(DropTarget::End, false, cx);
    let end = tab_dnd::drop_target(
        div().id("tab-end-drop").relative().flex_1().min_h(px(ROW_HEIGHT)).w_full(),
        DropTarget::End,
        false,
        cx,
    )
    .when(end_feedback != Feedback::None, |d| d.child(indicator(crate::tab_ops::Edge::Before, false, cx)));

    let list = div()
        .id("tab-list")
        .v_flex()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .v_flex()
                .min_h_full()
                .gap(px(2.))
                .px(px(8.))
                .pb(px(8.))
                .children(children)
                .child(end),
        );

    div()
        .id("vertical-tab-sidebar")
        .role(Role::Group)
        .aria_label("Vertical Tab Sidebar")
        .accessibility_id("Vertical Tab Sidebar")
        .relative()
        .v_flex()
        .w(px(workspace.sidebar_width))
        .h_full()
        .flex_none()
        .bg(palette.tint(0.045))
        .child(header(workspace, titlebar_height, cx))
        .child(list)
        .children(workspace.overlays.as_ref().map(|overlays| overlays.read(cx).quota_footer.clone()))
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(px(1.))
                .bg(palette.tint(0.1)),
        )
        .child(resize_handle(cx))
}

/// The sidebar's top row: empty under the titlebar, or with its own new-tab button when
/// the window has no titlebar.
fn header(workspace: &Workspace, titlebar_height: f32, cx: &mut Context<Workspace>) -> impl IntoElement {
    if titlebar_height > 0.0 {
        return div().h(px(titlebar_height)).flex_none().into_any_element();
    }
    div()
        .h_flex()
        .items_center()
        .justify_end()
        .h(px(TITLEBAR_HEIGHT))
        .flex_none()
        .pl(px(8.))
        .pr(px(10.))
        .child(div().flex_1().h_full().window_control_area(WindowControlArea::Drag))
        .child(new_tab_button(workspace, cx))
        .into_any_element()
}

fn resize_handle(cx: &mut Context<Workspace>) -> impl IntoElement {
    div()
        .id("sidebar-resize")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(px(HANDLE_WIDTH))
        .cursor_col_resize()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.chrome.resize =
                    Some(Resize { pointer: f32::from(event.position.x), width: this.sidebar_width });
                SidebarDragging::set(true, cx);
            }),
        )
        .on_drag(ResizeDrag, |_, _, _, cx| cx.new(|_| EmptyView))
}

struct EmptyView;

impl Render for EmptyView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_0()
    }
}

/// Applies a resize drag: the width follows the pointer, snapped to device pixels.
pub fn resize_to(workspace: &mut Workspace, pointer: Pixels, window: &Window, cx: &mut Context<Workspace>) {
    let Some(resize) = workspace.chrome.resize else { return };
    let scale = window.scale_factor();
    let width = resize.width + f32::from(pointer) - resize.pointer;
    let snapped = (width * scale).round() / scale;
    workspace.set_sidebar_width(snapped.clamp(SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH), cx);
}

fn title_text(title: &str, size: f32, weight: FontWeight, color: Hsla) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(size))
        .font_weight(weight)
        .text_color(color)
        .child(title.to_string())
}

pub fn icon(path: &'static str, size: f32, color: Hsla) -> Svg {
    svg().path(path).size(px(size)).flex_none().text_color(color)
}

fn tab_row(
    workspace: &mut Workspace,
    item: &TabItem<TabId>,
    palette: Palette,
    dragging: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let id = item.id;
    let key = RowKey::Tab(id);
    let hovered = !dragging && workspace.chrome.hovered == Some(key);
    let fades = &mut workspace.chrome.fades;
    let selected = fades.level(key, FadeKind::Selected, f32::from(u8::from(item.is_selected)), window);
    let hover = fades.level(key, FadeKind::Hover, f32::from(u8::from(hovered)), window);

    let fill = hover * 0.06 * (1.0 - selected) + selected * 0.12;
    let capsule = match item.color.and_then(|c| display_color(c, palette.dark)) {
        Some(color) => color,
        None if item.group.is_none() => palette.accent.opacity(selected),
        None => gpui_kit::transparent_black(),
    };
    let grouped = item.group.is_some();
    let feedback = workspace.drop_feedback(DropTarget::Tab(id), false, cx);
    let title_color = palette.primary.opacity(0.55 + 0.45 * selected);
    let weight = if item.is_selected { FontWeight::MEDIUM } else { FontWeight::NORMAL };
    let detail = item.detail();
    let popover_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
    let capture = popover_bounds.clone();
    let title = item.title.clone();
    let bell = item.has_bell;
    let group_color = item.group_color.and_then(|c| display_color(c, palette.dark));
    let weak = cx.entity().downgrade();

    let trailing = if hovered {
        div()
            .id(("tab-close", id))
            .role(Role::Button)
            .aria_label("Close Tab")
            .flex()
            .items_center()
            .justify_center()
            .size(px(18.))
            .flex_none()
            .rounded_full()
            .bg(palette.tint(0.1))
            .child(icon("ui/xmark.svg", 9., palette.secondary))
            .tooltip(|window, cx| Tooltip::new("Close Tab").build(window, cx))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.request_close_tab(id, window, cx);
            }))
            .into_any_element()
    } else if bell {
        icon("ui/bell-fill.svg", 10., palette.orange).into_any_element()
    } else {
        div().into_any_element()
    };

    let inner = div()
        .id(("tab-row", id))
        .role(Role::Button)
        .aria_label(if bell { format!("{title}, bell") } else { title.clone() })
        .aria_selected(item.is_selected)
        .aria_description(detail)
        .flex_1()
        .min_w_0()
        .h_flex()
        .items_center()
        .gap(px(8.))
        .h(px(ROW_HEIGHT))
        .pl(px(6.))
        .pr(px(8.))
        .rounded(px(8.))
        .bg(palette.tint(fill))
        .child(div().w(px(3.)).h(px(14.)).flex_none().rounded_full().bg(capsule))
        .child(div().flex_1().min_w_0().child(MiddleText::new(title.clone(), 13., weight, title_color)))
        .child(trailing)
        .on_prepaint(move |bounds, _, _| capture.set(bounds))
        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
            this.set_hovered(key, *hovered, cx);
            this.hover_tab(id, crate::tab_popover::Placement::Right, popover_bounds.get(), *hovered, window, cx);
        }))
        .on_click(cx.listener(move |this, _, window, cx| this.select_tab(id, window, cx)))
        .on_drag(Dragged::Tab(id), {
            let title = title.clone();
            move |_, _, _, cx| tab_dnd::preview(&title, 200., cx)
        });

    let outer = div()
        .id(("tab-slot", id))
        .relative()
        .h_flex()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .when(grouped, |d| d.pl(px(12.)))
        .child(inner)
        .when(grouped, |d| {
            d.child(
                div()
                    .absolute()
                    .left(px(6.))
                    .top(px(-1.))
                    .bottom(px(-1.))
                    .w(px(2.))
                    .rounded_full()
                    .bg(group_color.unwrap_or(palette.secondary.opacity(0.4) )),
            )
        })
        .when_some(edge_of(feedback), |d, edge| d.child(indicator(edge, false, cx)));

    tab_dnd::drop_target(outer, DropTarget::Tab(id), false, cx)
        .context_menu(move |_, cx| tab_menu(id, &weak, cx))
}

/// How assistive technology reads a group header: "Work, 3 tabs, expanded".
pub fn group_label(name: &str, count: usize, collapsed: bool) -> String {
    let tabs = if count == 1 { "1 tab".to_string() } else { format!("{count} tabs") };
    format!("{name}, {tabs}, {}", if collapsed { "collapsed" } else { "expanded" })
}

pub fn edge_of(feedback: Feedback) -> Option<crate::tab_ops::Edge> {
    match feedback {
        Feedback::Edge(edge) => Some(edge),
        _ => None,
    }
}

fn group_header(
    workspace: &mut Workspace,
    header: &GroupHeader,
    anchor: TabId,
    palette: Palette,
    dragging: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let group = header.id;
    let key = RowKey::Group(group, anchor);
    let target = DropTarget::Group { group, anchor, collapsed: header.is_collapsed };
    let hovered = !dragging && workspace.chrome.hovered == Some(key);
    let hover = workspace.chrome.fades.level(key, FadeKind::Hover, f32::from(u8::from(hovered)), window);
    let feedback = workspace.drop_feedback(target, false, cx);
    let renaming = workspace.chrome.renaming.as_ref().filter(|r| r.group == group).map(|r| r.input.clone());

    let resting: f32 = if header.is_collapsed && header.contains_selected { 0.12 } else { 0.0 };
    let fill = if feedback == Feedback::Join {
        palette.accent.opacity(0.25)
    } else {
        palette.tint(resting.max(hover * 0.06))
    };
    let color = header.color.and_then(|c| display_color(c, palette.dark)).unwrap_or(palette.secondary.opacity(0.4));
    let weak = cx.entity().downgrade();
    let (name, collapsed, count, bell) = (header.name.clone(), header.is_collapsed, header.count, header.has_bell);

    let label = match renaming {
        Some(input) => div()
            .flex_1()
            .min_w_0()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(Input::new(&input).appearance(false).bordered(false).focus_bordered(false).xsmall().p_0())
            .into_any_element(),
        None => title_text(&name, 12., FontWeight::SEMIBOLD, palette.primary).into_any_element(),
    };
    let is_renaming = workspace.chrome.renaming.as_ref().is_some_and(|r| r.group == group);

    let row = div()
        .id(("group-row", anchor))
        .role(Role::Button)
        .aria_label(group_label(&name, count, collapsed))
        .aria_expanded(!collapsed)
        .relative()
        .h_flex()
        .items_center()
        .gap(px(6.))
        .h(px(HEADER_HEIGHT))
        .flex_none()
        .px(px(8.))
        .rounded(px(8.))
        .bg(fill)
        .child(
            div().w(px(10.)).flex().justify_center().flex_none().child(
                icon("ui/chevron-right.svg", 9., palette.tertiary).when(!collapsed, |s| {
                    s.with_transformation(Transformation::rotate(percentage(0.25)))
                }),
            ),
        )
        .child(div().size(px(8.)).flex_none().rounded_full().bg(color))
        .child(label)
        .when(collapsed, |d| {
            d.when(bell, |d| d.child(icon("ui/bell-fill.svg", 10., palette.orange)))
                .child(div().text_size(px(11.)).text_color(palette.tertiary).child(count.to_string()))
        })
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| this.set_hovered(key, *hovered, cx)))
        .on_click(cx.listener(move |this, _, window, cx| {
            if !this.is_renaming(group) {
                this.toggle_collapsed(group, cx);
                this.focus_selected(window, cx);
            }
        }))
        .when(!is_renaming, |d| {
            d.on_drag(Dragged::Group { id: group, anchor }, {
                let name = name.clone();
                move |_, _, _, cx| tab_dnd::preview(&name, 200., cx)
            })
        })
        .when_some(edge_of(feedback), |d, edge| d.child(indicator(edge, false, cx)));

    tab_dnd::drop_target(row, target, false, cx)
        .context_menu(move |_, cx| group_menu(group, collapsed, &weak, cx))
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::group_label;

    #[test]
    fn group_labels_name_the_count_and_state() {
        assert_eq!(group_label("Work", 3, false), "Work, 3 tabs, expanded");
        assert_eq!(group_label("Docs", 1, true), "Docs, 1 tab, collapsed");
    }
}
