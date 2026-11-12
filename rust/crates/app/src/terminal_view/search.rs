//! The find bar's state and the search commands.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;

use super::TerminalView;
use crate::keybinds::SearchDirection;

pub struct SearchState {
    pub input: Entity<InputState>,
    /// Matches found so far.
    pub total: usize,
    /// Index of the selected match, 0 being the newest.
    pub selected: Option<usize>,
    _subscription: Subscription,
}

impl TerminalView {
    /// Opens the find bar, or refocuses it. The needle defaults to the
    /// current selection.
    pub(crate) fn start_search(&mut self, needle: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let needle =
            needle.or_else(|| self.session.term().selection_text().filter(|t| !t.is_empty() && !t.contains('\n')));
        if self.search.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
            let subscription = cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| match event {
                InputEvent::Change => {
                    let text = input.read(cx).value();
                    this.set_needle(&text, cx);
                }
                InputEvent::PressEnter { shift, .. } => {
                    let direction = if *shift { SearchDirection::Previous } else { SearchDirection::Next };
                    this.navigate_search(direction, cx);
                }
                InputEvent::Focus | InputEvent::Blur => {}
            });
            self.search = Some(SearchState { input, total: 0, selected: None, _subscription: subscription });
        }
        let Some(search) = &self.search else { return };
        let input = search.input.clone();
        if let Some(needle) = needle {
            self.set_needle(&needle, cx);
            input.update(cx, |input, cx| input.set_value(needle, window, cx));
        }
        input.update(cx, |input, cx| {
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    fn set_needle(&mut self, needle: &str, cx: &mut Context<Self>) {
        self.session.term().search_set(needle);
        if let Some(search) = &mut self.search {
            search.total = 0;
            search.selected = None;
        }
        cx.notify();
    }

    /// Closes the find bar and returns focus to the terminal. Returns whether
    /// it was open.
    pub(crate) fn end_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.search.take().is_none() {
            return false;
        }
        self.session.term().search_close();
        window.focus(&self.focus, cx);
        cx.notify();
        true
    }

    pub(crate) fn search_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.session.term().selection_text().filter(|t| !t.is_empty()) else { return false };
        self.start_search(Some(text), window, cx);
        true
    }

    pub(crate) fn navigate_search(&mut self, direction: SearchDirection, cx: &mut Context<Self>) -> bool {
        if self.search.is_none() {
            return false;
        }
        let moved = {
            let mut term = self.session.term();
            match direction {
                SearchDirection::Next => term.search_next(),
                SearchDirection::Previous => term.search_prev(),
            }
        };
        cx.notify();
        moved
    }
}
