//! The horizontal tab bar, shown while the sidebar is hidden and there is more than one tab.

use angelly_core::persist::{GroupHeader, Row, TabItem};
use gpui_kit::component::input::Input;
use gpui_kit::component::Sizable as _;
use crate::menu::ContextMenuExt as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::StyledExt as _;
use gpui_kit::*;
use gpui_kit::base::ElementExt as _;

use crate::chrome::{Dragged, DropTarget, FadeKind, RowKey};
use crate::middle_text::MiddleText;
use crate::sidebar::{edge_of, icon, Palette};
use crate::tab_colors::display_color;
use crate::tab_dnd::{self, indicator};
use crate::tab_menus::{group_menu, tab_menu};
use crate::tab_ops::Feedback;
use crate::theme::Theme;
use crate::titlebar::new_tab_button;
use crate::workspace::{TabId, Workspace};

pub const BAR_HEIGHT: f32 = 40.0;
const BUTTON_WIDTH: f32 = 168.0;
const BUTTON_HEIGHT: f32 = 32.0;
const COMPACT_HEIGHT: f32 = 32.0;
const COMPACT_BUTTON_HEIGHT: f32 = 26.0;

/// The tab bar. `compact` is the form that sits in the titlebar row (the `tabs` titlebar style).
pub fn render(workspace: &mut Workspace, compact: bool, window: &mut Window, cx: &mut Context<Workspace>) -> impl IntoElement {
    let bar_height = if compact { COMPACT_HEIGHT } else { BAR_HEIGHT };
    let button_height = if compact { COMPACT_BUTTON_HEIGHT } else { BUTTON_HEIGHT };
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

    let target = workspace.scroll_target(&rows);
    if target != workspace.chrome.scrolled_to {
        workspace.chrome.scrolled_to = target;
        if let Some(index) = target {
            workspace.chrome.tab_scroll.scroll_to_item(index);
        }
    }

    let mut children = Vec::new();
    for row in &rows {
        children.push(match row {
            Row::Tab(item) => tab_button(workspace, item, button_height, palette, window, cx).into_any_element(),
            Row::Group { header, anchor } => {
                group_button(workspace, header, *anchor, button_height, palette, cx).into_any_element()
            }
        });
    }
    let end_feedback = workspace.drop_feedback(DropTarget::End, true, cx);
    children.push(
        tab_dnd::drop_target(
            div().id("tab-bar-end").relative().flex_1().min_w(px(32.)).h_full(),
            DropTarget::End,
            true,
            cx,
        )
        .when(end_feedback != Feedback::None, |d| d.child(indicator(crate::tab_ops::Edge::Before, true, cx)))
        .into_any_element(),
    );

    let scroller = div()
        .id("tab-bar-scroll")
        .h_flex()
        .flex_1()
        .min_w_0()
        .h_full()
        .gap(px(3.))
        .px(px(6.))
        .py(px(4.))
        .overflow_x_scroll()
        .track_scroll(&workspace.chrome.tab_scroll)
        .children(children);

    let new_tab_visible = crate::prefs::Prefs::read(cx).new_tab_placement()
        == angelly_core::persist::HorizontalNewTabButtonPlacement::Tabbar;
    let new_tab_hover = workspace.drop_feedback(DropTarget::End, true, cx) != Feedback::None;
    let new_tab = tab_dnd::drop_target(
        div()
            .id("tab-bar-new-tab")
            .w(px(36.))
            .h(px(bar_height))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .when(new_tab_hover, |d| d.bg(palette.accent.opacity(0.15)))
            .child(new_tab_button(workspace, cx)),
        DropTarget::End,
        true,
        cx,
    );

    div()
        .relative()
        .h_flex()
        .w_full()
        .h(px(bar_height))
        .flex_none()
        .when(!compact, |bar| bar.bg(palette.tint(0.045)))
        .child(scroller)
        .when(new_tab_visible, |bar| bar.child(new_tab))
        .when(!compact, |bar| bar.child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(palette.tint(0.1))))
}

fn tab_button(
    workspace: &mut Workspace,
    item: &TabItem<TabId>,
    button_height: f32,
    palette: Palette,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let id = item.id;
    let key = RowKey::Tab(id);
    let selected = workspace.chrome.fades.level(key, FadeKind::Selected, f32::from(u8::from(item.is_selected)), window);
    let feedback = workspace.drop_feedback(DropTarget::Tab(id), true, cx);
    let fill = 0.035 + (0.12 - 0.035) * selected;
    let dot = item.color.and_then(|c| display_color(c, palette.dark));
    let group_color = item.group_color.and_then(|c| display_color(c, palette.dark));
    let detail = item.detail();
    let popover_bounds = std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
    let capture = popover_bounds.clone();
    let title = item.title.clone();
    let weak = cx.entity().downgrade();

    let body = div()
        .id(("tab-button", id))
        .role(Role::Button)
        .aria_label(if item.has_bell { format!("{title}, bell") } else { title.clone() })
        .aria_selected(item.is_selected)
        .aria_description(detail)
        .h_flex()
        .items_center()
        .gap(px(6.))
        .flex_1()
        .min_w_0()
        .h_full()
        .pl(px(10.))
        .when_some(dot, |d, color| d.child(div().size(px(6.)).flex_none().rounded_full().bg(color)))
        .child(div().flex_1().min_w_0().child(MiddleText::new(
            title.clone(),
            12.,
            if item.is_selected { FontWeight::MEDIUM } else { FontWeight::NORMAL },
            palette.primary,
        )))
        .when(item.has_bell, |d| d.child(icon("ui/bell-fill.svg", 10., palette.orange)))
        .on_click(cx.listener(move |this, _, window, cx| this.select_tab(id, window, cx)))
        .on_drag(Dragged::Tab(id), {
            let title = title.clone();
            move |_, _, _, cx| tab_dnd::preview(&title, BUTTON_WIDTH, cx)
        });

    let close = div()
        .id(("tab-button-close", id))
        .role(Role::Button)
        .aria_label("Close Tab")
        .flex()
        .items_center()
        .justify_center()
        .w(px(26.))
        .h_full()
        .flex_none()
        .child(icon("ui/xmark.svg", 9., palette.primary))
        .tooltip(|window, cx| Tooltip::new("Close Tab").build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| this.request_close_tab(id, window, cx)));

    let button = div()
        .id(("tab-slot", id))
        .relative()
        .h_flex()
        .w(px(BUTTON_WIDTH))
        .h(px(button_height))
        .flex_none()
        .rounded(px(6.))
        .overflow_hidden()
        .bg(palette.tint(fill))
        .child(body)
        .child(close)
        .when_some(group_color, |d, color| {
            d.child(div().absolute().left_0().right_0().bottom_0().h(px(3.)).bg(color))
        })
        .on_prepaint(move |bounds, _, _| capture.set(bounds))
        .on_hover(cx.listener(move |this, hovered: &bool, window, cx| {
            this.hover_tab(id, crate::tab_popover::Placement::Bottom, popover_bounds.get(), *hovered, window, cx);
        }))
        .when_some(edge_of(feedback), |d, edge| d.child(indicator(edge, true, cx)));

    tab_dnd::drop_target(button, DropTarget::Tab(id), true, cx)
        .context_menu(move |_, cx| tab_menu(id, &weak, cx))
}

fn group_button(
    workspace: &mut Workspace,
    header: &GroupHeader,
    anchor: TabId,
    button_height: f32,
    palette: Palette,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let group = header.id;
    let target = DropTarget::Group { group, anchor, collapsed: header.is_collapsed };
    let feedback = workspace.drop_feedback(target, true, cx);
    let renaming = workspace.chrome.renaming.as_ref().filter(|r| r.group == group).map(|r| r.input.clone());
    let color = header.color.and_then(|c| display_color(c, palette.dark)).unwrap_or(palette.secondary);
    let weak = cx.entity().downgrade();
    let (name, collapsed) = (header.name.clone(), header.is_collapsed);
    let tooltip = if collapsed { "Expand Group" } else { "Collapse Group" };

    let label = match &renaming {
        Some(input) => div()
            .w(px(120.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(Input::new(input).appearance(false).bordered(false).focus_bordered(false).xsmall().p_0())
            .into_any_element(),
        None => div()
            .max_w(px(160.))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.primary)
            .child(name.clone())
            .into_any_element(),
    };

    let button = div()
        .id(("group-button", anchor))
        .role(Role::Button)
        .aria_label(crate::sidebar::group_label(&name, header.count, collapsed))
        .aria_expanded(!collapsed)
        .relative()
        .h_flex()
        .items_center()
        .gap(px(6.))
        .h(px(button_height))
        .flex_none()
        .px(px(10.))
        .rounded(px(6.))
        .overflow_hidden()
        .bg(if feedback == Feedback::Join { color.opacity(0.25) } else { color.opacity(0.12) })
        .when(collapsed && header.contains_selected, |d| d.border_1().border_color(palette.primary.opacity(0.6)))
        .child(icon(
            if collapsed { "ui/chevron-right.svg" } else { "ui/chevron-down.svg" },
            9.,
            palette.primary,
        ))
        .child(label)
        .when(collapsed, |d| {
            d.when(header.has_bell, |d| d.child(icon("ui/bell-fill.svg", 10., palette.orange)))
                .child(div().text_size(px(11.)).text_color(palette.primary).child(header.count.to_string()))
        })
        .child(div().absolute().left_0().right_0().bottom_0().h(px(3.)).bg(color))
        .when(renaming.is_none(), |d| {
            d.tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.toggle_collapsed(group, cx);
                    this.focus_selected(window, cx);
                }))
                .on_drag(Dragged::Group { id: group, anchor }, {
                    let name = name.clone();
                    move |_, _, _, cx| tab_dnd::preview(&name, BUTTON_WIDTH, cx)
                })
        })
        .when_some(edge_of(feedback), |d, edge| d.child(indicator(edge, true, cx)));

    tab_dnd::drop_target(button, target, true, cx)
        .context_menu(move |_, cx| group_menu(group, collapsed, &weak, cx))
}

impl Workspace {
    /// The row to keep in view: the group being renamed, else the selected row.
    pub fn scroll_target(&self, rows: &[Row<TabId>]) -> Option<usize> {
        let renaming = self.chrome.renaming.as_ref().map(|r| r.group);
        let wanted = rows.iter().position(|row| match (row, renaming) {
            (Row::Group { header, .. }, Some(group)) => header.id == group,
            _ => false,
        });
        wanted.or_else(|| {
            rows.iter().position(|row| match row {
                Row::Tab(item) => item.is_selected,
                Row::Group { header, .. } => header.is_collapsed && header.contains_selected,
            })
        })
    }
}
