//! The command palette overlay.

use gpui_kit::component::input::{Escape, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::commands::{CommandAction, CommandOption};
use super::fuzzy::{filter_sorted, matched_indices};
use super::ui::{self, icon, text, Look};

pub enum PaletteEvent {
    Run(CommandAction),
    Dismissed,
}

impl EventEmitter<PaletteEvent> for Palette {}

/// Selection arithmetic from the Swift palette: no selection until the user
/// moves or types; moving wraps at both ends.
pub fn move_selection(current: Option<usize>, count: usize, up: bool) -> Option<usize> {
    if count == 0 {
        return current;
    }
    Some(if up {
        match current {
            None | Some(0) => count - 1,
            Some(i) => i.min(count) - 1,
        }
    } else {
        match current {
            None => 0,
            Some(i) if i + 1 >= count => 0,
            Some(i) => i + 1,
        }
    })
}

/// How the selection reacts to the query changing: a non-empty query selects
/// the first row; clearing it drops a selection that was still on the first.
pub fn selection_after_query(selected: Option<usize>, query_is_empty: bool) -> Option<usize> {
    match (query_is_empty, selected) {
        (false, None) => Some(0),
        (true, Some(0)) => None,
        (_, selected) => selected,
    }
}

pub struct Palette {
    options: Vec<CommandOption>,
    input: Entity<InputState>,
    query: String,
    filtered: Vec<usize>,
    selected: Option<usize>,
    scroll: ScrollHandle,
    focus: FocusHandle,
    _subscriptions: [Subscription; 2],
}

impl Palette {
    pub fn new(options: Vec<CommandOption>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = ui::new_input(window, cx, "Execute a command…", "");
        let subscription = cx.subscribe_in(&input, window, |this, input, event: &InputEvent, window, cx| match event {
            InputEvent::Change => {
                this.query = input.read(cx).value().trim().to_string();
                this.refilter();
                cx.notify();
            }
            InputEvent::PressEnter { .. } => this.submit(window, cx),
            _ => {}
        });
        let focus = cx.focus_handle();
        let focus_out = cx.on_focus_out(&focus, window, |_, _, _, cx| cx.emit(PaletteEvent::Dismissed));
        input.update(cx, |state, cx| state.focus(window, cx));
        let mut this = Self {
            filtered: (0..options.len()).collect(),
            options,
            input,
            query: String::new(),
            selected: None,
            scroll: ScrollHandle::new(),
            focus,
            _subscriptions: [subscription, focus_out],
        };
        this.refilter();
        this
    }

    fn refilter(&mut self) {
        self.filtered = if self.query.is_empty() {
            (0..self.options.len()).collect()
        } else {
            filter_sorted(&self.options, &self.query)
        };
        self.selected = selection_after_query(self.selected, self.query.is_empty());
    }

    fn selected_option(&self) -> Option<&CommandOption> {
        let index = self.selected?;
        self.filtered.get(index).or_else(|| self.filtered.last()).map(|&i| &self.options[i])
    }

    fn submit(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let action = self.selected_option().map(|o| o.action.clone());
        cx.emit(PaletteEvent::Dismissed);
        if let Some(action) = action {
            cx.emit(PaletteEvent::Run(action));
        }
    }

    fn run(&mut self, index: usize, cx: &mut Context<Self>) {
        let action = self.filtered.get(index).map(|&i| self.options[i].action.clone());
        cx.emit(PaletteEvent::Dismissed);
        if let Some(action) = action {
            cx.emit(PaletteEvent::Run(action));
        }
    }

    /// Replaces the query as if it had been typed.
    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |state, cx| state.set_value(query.to_string(), window, cx));
        self.query = query.trim().to_string();
        self.refilter();
        cx.notify();
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn step(&mut self, up: bool, cx: &mut Context<Self>) {
        self.selected = move_selection(self.selected, self.filtered.len(), up);
        if let Some(index) = self.selected {
            self.scroll.scroll_to_item(index.min(self.filtered.len().saturating_sub(1)));
        }
        cx.notify();
    }

    fn title(&self, option: &CommandOption, look: &Look) -> StyledText {
        let mut styled = StyledText::new(option.title.clone());
        if self.query.is_empty() {
            return styled;
        }
        if let Some(indices) = matched_indices(&option.title, &self.query) {
            let highlight = HighlightStyle { color: Some(look.accent), font_weight: Some(FontWeight::BOLD), ..Default::default() };
            styled = styled.with_highlights(char_ranges(&option.title, &indices).into_iter().map(|r| (r, highlight)));
        }
        styled
    }

    fn subtitle(&self, option: &CommandOption, look: &Look) -> Option<StyledText> {
        let detail = option.detail()?;
        let mut styled = StyledText::new(detail.to_string());
        if !self.query.is_empty() && matched_indices(&option.title, &self.query).is_none() {
            if let Some(indices) = matched_indices(detail, &self.query) {
                let highlight = HighlightStyle { color: Some(look.accent), font_weight: Some(FontWeight::BOLD), ..Default::default() };
                styled = styled.with_highlights(char_ranges(detail, &indices).into_iter().map(|r| (r, highlight)));
            }
        }
        Some(styled)
    }

    fn row(&self, position: usize, option: &CommandOption, look: &Look, cx: &mut Context<Self>) -> Stateful<Div> {
        let selected = self.selected.is_some_and(|s| s == position || (s >= self.filtered.len() && position + 1 == self.filtered.len()));
        let hover = look.secondary.opacity(0.2);
        let accent = look.accent;
        let title = self.title(option, look);
        let subtitle = self.subtitle(option, look);

        let mut text_column = div().v_flex().gap(px(2.)).min_w_0().child(
            div()
                .text_size(px(text::BODY))
                .font_weight(if option.emphasis { FontWeight::MEDIUM } else { FontWeight::NORMAL })
                .child(title),
        );
        if let Some(subtitle) = subtitle {
            text_column = text_column.child(div().text_size(px(text::CAPTION)).text_color(look.secondary).child(subtitle));
        }

        div()
            .id(("palette-row", position))
            .h_flex()
            .items_center()
            .gap(px(8.))
            .p(px(8.))
            .rounded(px(5.))
            .flex_none()
            .cursor_pointer()
            .when(selected, |d| d.bg(accent.opacity(0.2)))
            .when(!selected, |d| d.hover(move |s| s.bg(hover)))
            .when(option.emphasis && !selected, |d| d.border_1().border_color(accent.opacity(0.3)))
            .on_click(cx.listener(move |this, _, _, cx| this.run(position, cx)))
            .children(option.color.map(|color| div().size(px(8.)).flex_none().rounded_full().bg(color)))
            .children(option.icon.map(|name| {
                icon(name, 14., if option.emphasis { accent } else { look.secondary })
            }))
            .child(text_column)
            .child(div().flex_1())
            .children(option.badge.clone().filter(|b| !b.is_empty()).map(|badge| {
                div()
                    .px(px(7.))
                    .py(px(3.))
                    .rounded_full()
                    .bg(accent.opacity(0.15))
                    .text_color(accent)
                    .text_size(px(text::CAPTION))
                    .font_weight(FontWeight::MEDIUM)
                    .child(badge)
            }))
            .when(!option.symbols.is_empty(), |d| {
                d.child(
                    div().h_flex().gap(px(1.)).text_color(look.secondary).text_size(px(text::BODY)).children(
                        option.symbols.iter().map(|s| div().min_w(px(13.)).flex().justify_center().child(s.clone())),
                    ),
                )
            })
    }
}

/// Byte ranges of the given character indices, merging neighbours.
fn char_ranges(text: &str, indices: &[usize]) -> Vec<std::ops::Range<usize>> {
    let offsets: Vec<usize> = text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
    let mut ranges: Vec<std::ops::Range<usize>> = Vec::new();
    for &index in indices {
        let (Some(&start), Some(&end)) = (offsets.get(index), offsets.get(index + 1)) else { continue };
        match ranges.last_mut() {
            Some(last) if last.end == start => last.end = end,
            _ => ranges.push(start..end),
        }
    }
    ranges
}

impl Render for Palette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let rows: Vec<Stateful<Div>> = self
            .filtered
            .iter()
            .enumerate()
            .map(|(position, &index)| self.row(position, &self.options[index].clone(), &look, cx))
            .collect();

        let list = if rows.is_empty() {
            div().p(px(16.)).text_color(look.secondary).text_size(px(text::BODY)).child("No matches").into_any_element()
        } else {
            div()
                .id("palette-list")
                .v_flex()
                .gap(px(4.))
                .p(px(10.))
                .max_h(px(200. + 20.))
                .overflow_y_scroll()
                .track_scroll(&self.scroll)
                .children(rows)
                .into_any_element()
        };

        let background = look.background.blend(look.primary.opacity(if look.dark { 0.04 } else { 0.0 }));
        div()
            .id("command-palette")
            .key_context("CommandPalette")
            .track_focus(&self.focus)
            .v_flex()
            .w_full()
            .max_w(px(500.))
            .rounded(px(10.))
            .bg(background.opacity(0.96))
            .border_1()
            .border_color(look.primary.opacity(0.225))
            .shadow(vec![BoxShadow {
                color: black().opacity(0.4),
                offset: point(px(0.), px(12.)),
                blur_radius: px(32.),
                spread_radius: px(0.),
            inset: false,
            }])
            .overflow_hidden()
            .occlude()
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .on_action(cx.listener(|_, _: &Escape, _, cx| cx.emit(PaletteEvent::Dismissed)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.step(true, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.step(false, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                let control = event.keystroke.modifiers.control;
                match key {
                    "escape" => cx.emit(PaletteEvent::Dismissed),
                    "up" => this.step(true, cx),
                    "down" => this.step(false, cx),
                    "p" if control => this.step(true, cx),
                    "n" if control => this.step(false, cx),
                    _ => {}
                }
            }))
            .child(
                div()
                    .h(px(48.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .text_size(px(20.))
                    .font_weight(FontWeight::LIGHT)
                    .child(
                        ui::field(&self.input)
                            .appearance(false)
                            .bordered(false)
                            .focus_bordered(false),
                    ),
            )
            .child(ui::divider(&look))
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn moving_down_starts_at_the_top_and_wraps() {
        assert_eq!(move_selection(None, 3, false), Some(0));
        assert_eq!(move_selection(Some(0), 3, false), Some(1));
        assert_eq!(move_selection(Some(2), 3, false), Some(0));
    }

    #[test]
    fn moving_up_starts_at_the_bottom_and_wraps() {
        assert_eq!(move_selection(None, 3, true), Some(2));
        assert_eq!(move_selection(Some(2), 3, true), Some(1));
        assert_eq!(move_selection(Some(0), 3, true), Some(2));
    }

    #[test]
    fn moving_in_an_empty_list_changes_nothing() {
        assert_eq!(move_selection(None, 0, true), None);
        assert_eq!(move_selection(Some(4), 0, false), Some(4));
    }

    #[test]
    fn a_stale_selection_past_the_end_moves_from_the_last_row() {
        assert_eq!(move_selection(Some(9), 3, true), Some(2));
        assert_eq!(move_selection(Some(9), 3, false), Some(0));
    }

    #[test]
    fn typing_selects_the_first_row_and_clearing_deselects_it() {
        assert_eq!(selection_after_query(None, false), Some(0));
        assert_eq!(selection_after_query(Some(2), false), Some(2));
        assert_eq!(selection_after_query(Some(0), true), None);
        assert_eq!(selection_after_query(Some(2), true), Some(2));
    }

    #[test]
    fn highlight_ranges_are_byte_offsets_and_merge() {
        assert_eq!(char_ranges("héllo", &[1, 2]), vec![1..4]);
        assert_eq!(char_ranges("Reset Terminal", &[0, 6]), vec![0..1, 6..7]);
        assert!(char_ranges("abc", &[7]).is_empty());
    }
}
