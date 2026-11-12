//! Executing actions on a terminal view: the keybind actions that concern one
//! surface, clipboard handling, and the gpui actions of the menu bar.

use angelly_core::config::Action as ConfigAction;
use gpui_kit::*;
use terminal::{PasteError, ScreenPart, SelectionAdjust, TextFormat};

use super::{Copy, Paste, SelectAll, SelectionClipboard, TerminalEvent, TerminalView};
use crate::panels::ClipboardKind;
use crate::keybinds::{Adjust, AppAction, Command, CopyFormat, FileDelivery, FileSource, Perform, SurfaceAction};
use crate::platform::menus::{
    DecreaseFontSize, Find, FindNext, FindPrevious, HideFindBar, IncreaseFontSize, PasteSelection, ResetFontSize,
    ToggleReadonly, UseSelectionForFind,
};

const MIN_FONT_SIZE: f32 = 1.0;
const MAX_FONT_SIZE: f32 = 255.0;

impl TerminalView {
    /// Runs one action of a binding. Returns whether it was carried out.
    pub(crate) fn run_command(
        &mut self,
        command: Command,
        action: &ConfigAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match command {
            Command::Ignore => true,
            Command::Send(bytes) => {
                if !self.readonly {
                    self.typed(cx);
                    self.session.write(&bytes);
                }
                true
            }
            Command::Surface(a) => self.run_surface(a, window, cx),
            Command::App(a) => self.run_app(a, action, window, cx),
            Command::Table(t) => {
                self.keys.apply(&t);
                true
            }
        }
    }

    /// Window, tab and split actions belong to the ancestors of the view:
    /// platform actions first, then the shared [`Perform`] action.
    fn run_app(&mut self, app: AppAction, action: &ConfigAction, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(platform) = crate::platform::actions::lookup(&action.to_string()) {
            if window.is_action_available(&*platform, cx) {
                window.dispatch_action(platform, cx);
                return true;
            }
        }
        let perform = Perform(app);
        if window.is_action_available(&perform, cx) {
            window.dispatch_action(Box::new(perform), cx);
            return true;
        }
        false
    }

    fn run_surface(&mut self, action: SurfaceAction, window: &mut Window, cx: &mut Context<Self>) -> bool {
        use SurfaceAction as S;
        match action {
            S::Reset => {
                self.session.term().reset();
                true
            }
            S::CopyToClipboard(format) => self.copy_selection_as(format, cx),
            S::PasteFromClipboard => {
                self.paste_clipboard(cx);
                true
            }
            S::PasteFromSelection => {
                self.paste_selection_clipboard(cx);
                true
            }
            S::CopyUrlToClipboard => match self.hover_link.as_ref().map(|l| l.text().to_string()) {
                Some(url) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(url));
                    true
                }
                None => false,
            },
            S::CopyTitleToClipboard => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.title()));
                true
            }
            S::IncreaseFontSize(points) => self.change_font_size(points, cx),
            S::DecreaseFontSize(points) => self.change_font_size(-points, cx),
            S::ResetFontSize => {
                self.set_font_size(self.settings.font_size, cx);
                true
            }
            S::SetFontSize(size) => {
                self.set_font_size(size, cx);
                true
            }
            S::Search(needle) => {
                self.start_search(Some(needle), window, cx);
                true
            }
            S::SearchSelection => self.search_selection(window, cx),
            S::NavigateSearch(direction) => self.navigate_search(direction, cx),
            S::StartSearch => {
                self.start_search(None, window, cx);
                true
            }
            S::EndSearch => self.end_search(window, cx),
            S::ClearScreen => {
                self.clear_screen(cx);
                true
            }
            S::SelectAll => {
                let selected = self.session.term().select_all();
                cx.notify();
                selected
            }
            S::ScrollToTop => {
                self.scroll(cx, |t| t.scroll_to_top());
                true
            }
            S::ScrollToBottom => {
                self.scroll(cx, |t| t.scroll_to_bottom());
                true
            }
            S::ScrollToSelection => {
                let moved = self.session.term().scroll_to_selection();
                self.scroller.poke();
                cx.notify();
                moved
            }
            S::ScrollToRow(row) => {
                self.scroll(cx, |t| t.scroll_to_row(row));
                true
            }
            S::ScrollPage(pages) => {
                let rows = f32::from(self.layout.grid.1);
                self.scroll(cx, |t| t.scroll_lines((pages * rows).round() as isize));
                true
            }
            S::ScrollLines(lines) => {
                self.scroll(cx, |t| t.scroll_lines(isize::from(lines)));
                true
            }
            S::AdjustSelection(adjust) => {
                let moved = match adjust {
                    Adjust::LeftWord => self.session.term().adjust_selection_by_word(false, &self.settings.word_boundaries),
                    Adjust::RightWord => self.session.term().adjust_selection_by_word(true, &self.settings.word_boundaries),
                    other => self.session.term().adjust_selection(selection_adjust(other)),
                };
                cx.notify();
                moved
            }
            S::JumpToPrompt(delta) => {
                self.scroll(cx, |t| t.scroll_to_prompt(isize::from(delta)));
                true
            }
            S::WriteFile(source, delivery, format) => self.write_file(source, delivery, format, cx),
            S::ToggleReadonly => {
                self.toggle_readonly(cx);
                true
            }
            S::ToggleMouseReporting => {
                self.mouse_reporting_enabled = !self.mouse_reporting_enabled;
                true
            }
            S::SetSurfaceTitle(title) => {
                self.title_override = (!title.is_empty()).then_some(title);
                cx.emit(TerminalEvent::Title(self.title()));
                cx.notify();
                true
            }
        }
    }

    fn scroll(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut terminal::Term)) {
        f(&mut self.session.term());
        self.scroller.poke();
        cx.notify();
    }

    pub(crate) fn toggle_readonly(&mut self, cx: &mut Context<Self>) {
        self.readonly = !self.readonly;
        cx.emit(TerminalEvent::ReadOnlyChanged(self.readonly));
        cx.notify();
    }

    fn change_font_size(&mut self, delta: f32, cx: &mut Context<Self>) -> bool {
        self.set_font_size(self.font_size + delta, cx);
        true
    }

    pub(crate) fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
        if size != self.font_size {
            self.font_size = size;
            self.invalidate_metrics();
            cx.notify();
        }
    }

    /// Erases the screen and scrollback. At a shell prompt the shell redraws
    /// itself (Ctrl-L); otherwise the terminal is cleared directly.
    fn clear_screen(&mut self, cx: &mut Context<Self>) {
        let mut term = self.session.term();
        if self.at_prompt && !self.readonly {
            term.write(b"\x1b[3J");
            drop(term);
            self.session.write(b"\x0c");
        } else {
            term.write(b"\x1b[H\x1b[2J\x1b[3J");
        }
        cx.notify();
    }

    fn selected_text(&self) -> Option<String> {
        self.selection_in(TextFormat::Plain)
    }

    fn selection_in(&self, format: TextFormat) -> Option<String> {
        let trim = self.settings.clipboard_trim_trailing_spaces;
        self.session.term().selection_text_as(format, trim).filter(|t| !t.is_empty())
    }

    /// Copies the selection to the clipboard. Returns whether there was one.
    pub(crate) fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        self.copy_selection_as(CopyFormat::Plain, cx)
    }

    fn copy_selection_as(&mut self, format: CopyFormat, cx: &mut Context<Self>) -> bool {
        let Some(plain) = self.selected_text() else { return false };
        match format {
            CopyFormat::Plain => cx.write_to_clipboard(ClipboardItem::new_string(plain)),
            CopyFormat::Vt => {
                let vt = self.selection_in(TextFormat::Vt).unwrap_or(plain);
                cx.write_to_clipboard(ClipboardItem::new_string(vt));
            }
            CopyFormat::Html | CopyFormat::Mixed => {
                let html = self.selection_in(TextFormat::Html).unwrap_or_default();
                let with_plain = (format == CopyFormat::Mixed).then_some(plain.as_str());
                if !super::macos::write_rich_clipboard(with_plain, &html) {
                    cx.write_to_clipboard(ClipboardItem::new_string(if format == CopyFormat::Html { html } else { plain }));
                }
            }
        }
        if self.settings.selection_clear_on_copy {
            self.session.term().clear_selection();
        }
        cx.notify();
        true
    }

    pub(crate) fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.paste_text(text, cx);
        }
    }

    pub(crate) fn paste_selection_clipboard(&mut self, cx: &mut Context<Self>) {
        let text = cx.try_global::<SelectionClipboard>().and_then(|s| s.0.clone()).or_else(|| self.selected_text());
        if let Some(text) = text {
            self.paste_text(text, cx);
        }
    }

    pub(crate) fn paste_text(&mut self, text: String, cx: &mut Context<Self>) {
        if self.readonly || text.is_empty() {
            return;
        }
        match self.try_paste(&text, !self.settings.paste_protection, cx) {
            Err(PasteError::Unsafe) => self.confirm_paste(text, cx),
            Err(PasteError::Failed(_)) | Ok(()) => {}
        }
    }

    fn try_paste(&mut self, text: &str, allow_unsafe: bool, cx: &mut Context<Self>) -> Result<(), PasteError> {
        let bytes = self.session.term().paste(text, allow_unsafe)?;
        self.typed(cx);
        self.session.write(&bytes);
        Ok(())
    }

    fn confirm_paste(&mut self, text: String, cx: &mut Context<Self>) {
        let shown = text.clone();
        self.ask_clipboard(ClipboardKind::Paste, shown, cx, move |this, cx| {
            let _ = this.try_paste(&text, true, cx);
        });
    }

    /// Writes terminal text to a temporary file and copies, pastes or opens
    /// its path.
    fn write_file(
        &mut self,
        source: FileSource,
        delivery: FileDelivery,
        format: TextFormat,
        cx: &mut Context<Self>,
    ) -> bool {
        let text = match source {
            FileSource::Screen => self.session.term().screen_text(ScreenPart::Screen, format),
            FileSource::Scrollback => self.session.term().screen_text(ScreenPart::Scrollback, format),
            FileSource::Selection => self.session.term().selection_text_as(format, false),
        };
        let Some(text) = text else { return false };
        let extension = if format == TextFormat::Html { "html" } else { "txt" };
        let path = std::env::temp_dir().join(format!("angelly-{}.{extension}", uuid::Uuid::new_v4()));
        if write_private(&path, &text).is_err() {
            return false;
        }
        let shown = path.to_string_lossy().into_owned();
        match delivery {
            FileDelivery::Copy => cx.write_to_clipboard(ClipboardItem::new_string(shown)),
            FileDelivery::Paste => self.paste_text(shown, cx),
            FileDelivery::Open => {
                if let Ok(url) = url::Url::from_file_path(&path) {
                    cx.open_url(url.as_str());
                }
            }
        }
        true
    }

    pub(crate) fn copy_action(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.copy_selection(cx) {
            cx.propagate();
        }
    }

    pub(crate) fn paste_action(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_clipboard(cx);
    }

    pub(crate) fn select_all_action(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.session.term().select_all();
        cx.notify();
    }

    /// Registers the handlers for the menu bar actions this view answers.
    pub(crate) fn menu_actions(&self, root: Stateful<Div>, cx: &mut Context<Self>) -> Stateful<Div> {
        root.on_action(cx.listener(|this, _: &PasteSelection, _, cx| this.paste_selection_clipboard(cx)))
            .on_action(cx.listener(|this, _: &Find, window, cx| this.start_search(None, window, cx)))
            .on_action(cx.listener(|this, _: &FindNext, _, cx| {
                this.navigate_search(crate::keybinds::SearchDirection::Next, cx);
            }))
            .on_action(cx.listener(|this, _: &FindPrevious, _, cx| {
                this.navigate_search(crate::keybinds::SearchDirection::Previous, cx);
            }))
            .on_action(cx.listener(|this, _: &HideFindBar, window, cx| {
                this.end_search(window, cx);
            }))
            .on_action(cx.listener(|this, _: &UseSelectionForFind, window, cx| {
                this.search_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &IncreaseFontSize, _, cx| {
                this.change_font_size(1.0, cx);
            }))
            .on_action(cx.listener(|this, _: &DecreaseFontSize, _, cx| {
                this.change_font_size(-1.0, cx);
            }))
            .on_action(cx.listener(|this, _: &ResetFontSize, _, cx| this.set_font_size(this.settings.font_size, cx)))
            .on_action(cx.listener(|this, _: &ToggleReadonly, _, cx| this.toggle_readonly(cx)))
    }
}

/// Creates `path` readable by the user only: dumps of the screen may hold secrets.
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(text.as_bytes())
}

fn selection_adjust(a: Adjust) -> SelectionAdjust {
    match a {
        Adjust::Left => SelectionAdjust::Left,
        Adjust::Right => SelectionAdjust::Right,
        Adjust::Up => SelectionAdjust::Up,
        Adjust::Down => SelectionAdjust::Down,
        Adjust::PageUp => SelectionAdjust::PageUp,
        Adjust::PageDown => SelectionAdjust::PageDown,
        Adjust::Home => SelectionAdjust::Home,
        Adjust::End => SelectionAdjust::End,
        Adjust::BeginningOfLine => SelectionAdjust::LineStart,
        Adjust::EndOfLine => SelectionAdjust::LineEnd,
        Adjust::LeftWord | Adjust::RightWord => unreachable!("word moves have no cell adjustment"),
    }
}
