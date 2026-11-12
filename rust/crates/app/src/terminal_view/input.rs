//! Keyboard input: keybind dispatch, key encoding for the child process and
//! the input method (IME, dead keys, composition) integration.
//!
//! On macOS a key press reaches the view in one of two ways. Keys that cannot
//! be part of a composition arrive in [`TerminalView::on_key_down`] first. Keys
//! the input method wants (while composing, dead keys, text from a non-ASCII
//! input source) reach [`EntityInputHandler`] first and come back to
//! `on_key_down` only when the input method declines them.

use std::ops::Range;

use angelly_core::config::Binding;
use gpui_kit::*;
use terminal::{Key, KeyAction, KeyEvent, Mods, OptionAsAlt};

use super::macos;
use super::TerminalView;
use crate::keybinds::{Chord, Command, Outcome};

/// A key press held back by an unfinished keybind sequence, replayed as
/// ordinary input when the sequence is abandoned.
#[derive(Clone, Debug)]
pub struct QueuedKey {
    pub keystroke: Keystroke,
    pub is_held: bool,
    /// The platform's virtual key code, naming the key by position.
    pub key_code: Option<u16>,
}

/// Text the input method is composing, drawn at the cursor but not yet sent
/// to the child.
#[derive(Clone, Debug, PartialEq)]
pub struct Preedit {
    pub text: String,
    /// Selected part of the text in UTF-16 units; empty for a plain caret.
    pub selection: Range<usize>,
}

/// Terminal cells the character occupies.
pub fn char_cells(c: char) -> usize {
    let cp = u32::from(c);
    let wide = matches!(
        cp,
        0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD
    );
    if wide {
        2
    } else if matches!(cp, 0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F) {
        0
    } else {
        1
    }
}

impl Preedit {
    fn cells_before(&self, utf16: usize) -> usize {
        let mut units = 0;
        let mut cells = 0;
        for c in self.text.chars() {
            if units >= utf16 {
                break;
            }
            units += c.len_utf16();
            cells += char_cells(c);
        }
        cells
    }

    pub fn cells(&self) -> usize {
        self.text.chars().map(char_cells).sum()
    }

    pub fn caret_cells(&self) -> usize {
        self.cells_before(self.selection.end)
    }

    pub fn selected_cells(&self) -> Option<(usize, usize)> {
        (!self.selection.is_empty())
            .then(|| (self.cells_before(self.selection.start), self.cells_before(self.selection.end)))
    }

    fn utf16_len(&self) -> usize {
        self.text.encode_utf16().count()
    }
}

fn mods_of(ks: &Keystroke, chord: &Chord) -> Mods {
    let mut mods = Mods::default();
    for (on, flag) in [
        (chord.mods.shift, Mods::SHIFT),
        (chord.mods.ctrl, Mods::CTRL),
        (chord.mods.alt, Mods::ALT),
        (chord.mods.super_key, Mods::SUPER),
    ] {
        if on {
            mods |= flag;
        }
    }
    if ks.modifiers.alt && macos::right_option_down() {
        mods |= Mods::ALT_SIDE;
    }
    mods
}

impl TerminalView {
    pub(crate) fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.key_down(ev.keystroke.clone(), ev.is_held, macos::current_key_code(), window, cx);
    }

    pub(crate) fn key_down(
        &mut self,
        keystroke: Keystroke,
        is_held: bool,
        key_code: Option<u16>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = QueuedKey { keystroke, is_held, key_code };
        if self.process_key(key, window, cx) {
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_key_up(&mut self, ev: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.readonly || self.session.term().kitty_keyboard_flags() == 0 {
            return;
        }
        if self.write_key(&ev.keystroke, macos::current_key_code(), KeyAction::Release, cx) {
            cx.stop_propagation();
        }
    }

    pub(crate) fn on_modifiers_changed(&mut self, ev: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.mouse.mods == ev.modifiers {
            return;
        }
        self.mouse.mods = ev.modifiers;
        self.refresh_link(cx);
        cx.notify();
    }

    /// Runs one key press through the keybinds. Returns whether the view took
    /// the key; when it did not, the input method may still get it.
    fn process_key(&mut self, key: QueuedKey, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let keybinds = self.keybinds.clone();
        let chord = Chord::from_keystroke(&key.keystroke, key.key_code);
        loop {
            match self.keys.process(&keybinds, chord.clone(), key.clone()) {
                Outcome::Match(binding) => {
                    self.run_binding(binding, &key, window, cx);
                    cx.notify();
                    return true;
                }
                Outcome::Pending => {
                    cx.notify();
                    return true;
                }
                Outcome::Abort(held) => {
                    for k in held {
                        self.send_key(&k, cx);
                    }
                }
                Outcome::Unbound => return self.send_key(&key, cx),
            }
        }
    }

    /// Performs a matched binding and, for `unconsumed:` bindings or ones that
    /// could not run (`performable:`), lets the key through to the child.
    fn run_binding(&mut self, binding: &Binding, key: &QueuedKey, window: &mut Window, cx: &mut Context<Self>) {
        let mut performed = false;
        for action in std::iter::once(&binding.action).chain(&binding.chained) {
            let Some(command) = Command::parse(action) else { continue };
            performed |= self.run_command(command, action, window, cx);
        }
        let passthrough = !binding.flags.consumed || (binding.flags.performable && !performed);
        if passthrough {
            self.send_key(key, cx);
        }
    }

    /// Encodes a key press for the child. Returns whether it was consumed.
    fn send_key(&mut self, key: &QueuedKey, cx: &mut Context<Self>) -> bool {
        let action = if key.is_held { KeyAction::Repeat } else { KeyAction::Press };
        self.write_key(&key.keystroke, key.key_code, action, cx)
    }

    fn write_key(&mut self, ks: &Keystroke, key_code: Option<u16>, action: KeyAction, cx: &mut Context<Self>) -> bool {
        if self.exit.is_waiting() {
            if action != KeyAction::Release && !self.readonly {
                self.exit.dismiss();
                cx.notify();
            }
            return true;
        }
        let chord = Chord::from_keystroke(ks, key_code);
        let Some(code) = Key::from_name(&chord.base) else { return false };
        let mods = mods_of(ks, &chord);
        let text = ks.key_char.as_deref().filter(|t| t.chars().all(|c| !c.is_control())).unwrap_or("");
        if action != KeyAction::Release
            && ks.modifiers.alt
            && text.is_empty()
            && !self.option_is_alt(mods)
            && chord.base.chars().count() == 1
        {
            // A dead key (Option-E on a US layout): the input method composes it.
            return false;
        }
        let consumed = if !text.is_empty() && mods.contains(Mods::SHIFT) { Mods::SHIFT } else { Mods::default() };
        let mut event = KeyEvent::press(code, mods).with_text(text).with_action(action).with_consumed(consumed);
        let mut single = chord.base.chars();
        if let (Some(c), None) = (single.next(), single.next()) {
            event = event.with_unshifted(c);
        }
        let bytes = self.session.term().encode_key(&event);
        if self.readonly {
            return true;
        }
        if bytes.is_empty() {
            return !text.is_empty() || code_is_modifier(code);
        }
        if action != KeyAction::Release {
            self.typed(cx);
        }
        self.session.write(&bytes);
        true
    }

    /// Whether this Option press acts as Alt (`macos-option-as-alt`) rather
    /// than composing characters.
    fn option_is_alt(&self, mods: Mods) -> bool {
        match self.settings.option_as_alt {
            OptionAsAlt::True => true,
            OptionAsAlt::Left => !mods.contains(Mods::ALT_SIDE),
            OptionAsAlt::Right => mods.contains(Mods::ALT_SIDE),
            OptionAsAlt::False => false,
        }
    }

    /// Bookkeeping for output the user typed: follow the live screen, drop the
    /// selection and restart the cursor blink.
    pub(crate) fn typed(&mut self, cx: &mut Context<Self>) {
        let mut term = self.session.term();
        if self.settings.scroll_to_bottom.keystroke && !term.viewport_at_bottom() {
            term.scroll_to_bottom();
        }
        if self.settings.selection_clear_on_typing && term.has_selection() {
            term.clear_selection();
        }
        drop(term);
        self.note_input(cx);
        cx.notify();
    }
}

fn code_is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::MetaLeft
            | Key::MetaRight
    )
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let preedit = self.preedit.as_ref()?;
        let units: Vec<u16> = preedit.text.encode_utf16().collect();
        let range = range.start.min(units.len())..range.end.min(units.len());
        *adjusted = Some(range.clone());
        Some(String::from_utf16_lossy(&units[range]))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let range = self.preedit.as_ref().map_or(0..0, |p| p.selection.clone());
        Some(UTF16Selection { range, reversed: false })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.preedit.as_ref().map(|p| 0..p.utf16_len())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.preedit.take().is_some() {
            cx.notify();
        }
    }

    fn replace_text_in_range(&mut self, _: Option<Range<usize>>, text: &str, _: &mut Window, cx: &mut Context<Self>) {
        self.preedit = None;
        cx.notify();
        let text: String = text.chars().filter(|c| !c.is_control() || *c == '\n').collect();
        if text.is_empty() || self.readonly {
            return;
        }
        self.typed(cx);
        self.session.write(text.as_bytes());
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        selection: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.preedit = (!text.is_empty()).then(|| {
            let end = text.encode_utf16().count();
            Preedit { text: text.to_string(), selection: selection.unwrap_or(end..end) }
        });
        self.note_input(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let metrics = self.metrics?;
        let (col, row) = self.frame.cursor.pos?;
        let before = self.preedit.as_ref().map_or(0, |p| p.cells_before(range.start));
        let width = self.preedit.as_ref().map_or(1, |p| p.cells_before(range.end).saturating_sub(before).max(1));
        let origin = point(
            self.layout.origin.x + metrics.cell_width * (f32::from(col) + before as f32),
            self.layout.origin.y + metrics.cell_height * f32::from(row),
        );
        Some(Bounds { origin, size: size(metrics.cell_width * width as f32, metrics.cell_height) })
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn wide_and_combining_widths() {
        assert_eq!(char_cells('a'), 1);
        assert_eq!(char_cells('漢'), 2);
        assert_eq!(char_cells('한'), 2);
        assert_eq!(char_cells('\u{301}'), 0);
    }

    #[test]
    fn preedit_cell_positions_follow_utf16_offsets() {
        let p = Preedit { text: "a漢b".to_string(), selection: 1..2 };
        assert_eq!(p.cells(), 4);
        assert_eq!(p.caret_cells(), 3);
        assert_eq!(p.selected_cells(), Some((1, 3)));
        assert_eq!(p.utf16_len(), 3);
        let caret = Preedit { text: "ab".to_string(), selection: 2..2 };
        assert_eq!(caret.selected_cells(), None);
        assert_eq!(caret.caret_cells(), 2);
    }
}
