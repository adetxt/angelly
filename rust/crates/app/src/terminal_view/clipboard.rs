//! Clipboard requests from the running program (OSC 52), mediated by the `clipboard-read` and
//! `clipboard-write` settings: allowed, denied, or put to the user in the window's clipboard
//! sheet.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use angelly_core::config::ClipboardAccess;
use gpui_kit::*;
use terminal::ClipboardLocation;

use super::{SelectionClipboard, TerminalEvent, TerminalView};
use crate::panels::{ClipboardKind, ClipboardRequest};
use crate::theme::Theme;

/// A question for the window's clipboard sheet, handed to whichever view shows it. Only the
/// first taker gets it; an untaken prompt counts as denied.
#[derive(Clone)]
pub struct ClipboardPrompt(Rc<RefCell<Option<ClipboardRequest>>>);

impl ClipboardPrompt {
    fn new(request: ClipboardRequest) -> Self {
        Self(Rc::new(RefCell::new(Some(request))))
    }

    pub fn take(&self) -> Option<ClipboardRequest> {
        self.0.borrow_mut().take()
    }
}

impl PartialEq for ClipboardPrompt {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ClipboardPrompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClipboardPrompt")
    }
}

impl TerminalView {
    /// OSC 52 write. `None` clears the clipboard.
    pub(super) fn osc52_write(&mut self, location: ClipboardLocation, text: Option<String>, cx: &mut Context<Self>) {
        let text = text.unwrap_or_default();
        match cx.global::<Theme>().config.clipboard_write() {
            ClipboardAccess::Allow => store(location, text, cx),
            ClipboardAccess::Deny => {}
            ClipboardAccess::Ask => {
                let shown = text.clone();
                self.ask_clipboard(ClipboardKind::Osc52Write, shown, cx, move |_, cx| store(location, text, cx));
            }
        }
    }

    /// OSC 52 read: the program gets the clipboard text as the reply to its query.
    pub(super) fn osc52_read(&mut self, location: ClipboardLocation, cx: &mut Context<Self>) {
        match cx.global::<Theme>().config.clipboard_read() {
            ClipboardAccess::Allow => {
                let text = load(location, cx);
                self.answer_clipboard_read(location, &text);
            }
            ClipboardAccess::Deny => {}
            ClipboardAccess::Ask => {
                let text = load(location, cx);
                let shown = text.clone();
                self.ask_clipboard(ClipboardKind::Osc52Read, shown, cx, move |this, _| {
                    this.answer_clipboard_read(location, &text);
                });
            }
        }
    }

    fn answer_clipboard_read(&mut self, location: ClipboardLocation, text: &str) {
        self.session.write(&terminal::clipboard_read_reply(location, text));
    }

    pub(super) fn ask_clipboard(
        &mut self,
        kind: ClipboardKind,
        contents: String,
        cx: &mut Context<Self>,
        allowed: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        let (answer, response) = async_channel::bounded(1);
        let request = ClipboardRequest {
            kind,
            contents,
            can_remember: false,
            image: None,
            completion: Box::new(move |confirmed, _| {
                answer.try_send(confirmed).ok();
            }),
        };
        cx.emit(TerminalEvent::Clipboard(ClipboardPrompt::new(request)));
        cx.spawn(async move |this, cx| {
            if response.recv().await == Ok(true) {
                this.update(cx, |this, cx| allowed(this, cx)).ok();
            }
        })
        .detach();
    }
}

/// The standard clipboard is the system's; the selection and primary clipboards are the app's
/// own, the one `copy-on-select = true` fills and `paste_from_selection` reads.
fn store(location: ClipboardLocation, text: String, cx: &mut App) {
    match location {
        ClipboardLocation::Standard => cx.write_to_clipboard(ClipboardItem::new_string(text)),
        ClipboardLocation::Selection | ClipboardLocation::Primary => cx.set_global(SelectionClipboard(Some(text))),
    }
}

fn load(location: ClipboardLocation, cx: &App) -> String {
    match location {
        ClipboardLocation::Standard => cx.read_from_clipboard().and_then(|item| item.text()),
        ClipboardLocation::Selection | ClipboardLocation::Primary => {
            cx.try_global::<SelectionClipboard>().and_then(|s| s.0.clone())
        }
    }
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn a_prompt_is_taken_once() {
        let prompt = ClipboardPrompt::new(ClipboardRequest {
            kind: ClipboardKind::Osc52Read,
            contents: String::new(),
            can_remember: false,
            image: None,
            completion: Box::new(|_, _| {}),
        });
        let copy = prompt.clone();
        assert_eq!(prompt, copy);
        assert!(copy.take().is_some());
        assert!(prompt.take().is_none());
    }
}
