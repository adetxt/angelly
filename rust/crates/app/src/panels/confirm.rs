//! Modal alerts: the generic confirm dialog other panels use for "close tab",
//! "quit" and similar questions, and the clipboard confirmation sheet.

use std::sync::Arc;

use angelly_core::update::ReleaseInfo;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::assets::app_icon;
use super::format::{abbreviated_date, civil};
use super::ui::{self, icon, text, Look, Variant};

type Completion = Box<dyn FnOnce(Option<String>, &mut Window, &mut App)>;

/// A question for the user. The first button is the default (Return), the
/// cancel button answers Escape.
pub struct Confirm {
    pub title: String,
    pub message: Option<String>,
    pub details: Vec<(String, String)>,
    pub confirm_label: String,
    /// `None` makes this a one-button notice.
    pub cancel_label: Option<String>,
    /// A text field, pre-filled with this value; its text is passed to `on_confirm`.
    pub input: Option<String>,
    pub on_confirm: Completion,
}

impl Confirm {
    pub fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        confirm_label: impl Into<String>,
        on_confirm: impl FnOnce(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            message: Some(message.into()),
            details: Vec::new(),
            confirm_label: confirm_label.into(),
            cancel_label: Some("Cancel".to_string()),
            input: None,
            on_confirm: Box::new(move |_, window, cx| on_confirm(window, cx)),
        }
    }

    /// A one-button message.
    pub fn notice(title: impl Into<String>, message: impl Into<String>) -> Self {
        let mut confirm = Self::new(title, message, "OK", |_, _| {});
        confirm.cancel_label = None;
        confirm
    }

    pub fn close_terminal(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Close Terminal?",
            "The terminal still has a running process. If you close the terminal the process will be killed.",
            "Close",
            on_confirm,
        )
    }

    pub fn close_tab(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Close Tab?",
            "The terminal still has a running process. If you close the tab the process will be killed.",
            "Close",
            on_confirm,
        )
    }

    pub fn close_other_tabs(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Close Other Tabs?",
            "At least one other tab still has a running process. If you close the tab the process will be killed.",
            "Close",
            on_confirm,
        )
    }

    pub fn close_tabs_to_right(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Close Tabs on the Right?",
            "At least one tab to the right still has a running process. If you close the tab the process will be killed.",
            "Close",
            on_confirm,
        )
    }

    pub fn close_group(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Close Group?",
            "At least one tab in this group has a running process. Closing the group will terminate its sessions.",
            "Close",
            on_confirm,
        )
    }

    pub fn close_all_windows(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Close All Windows?",
            "All terminal sessions will be terminated.",
            "Close All Windows",
            on_confirm,
        )
    }

    pub fn quit(on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Quit Ghostty?",
            "The terminal still has a running process. If you quit, the process will be killed.",
            "Terminate",
            on_confirm,
        )
    }

    pub fn remove_worktree(path: &str, on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        Self::new(
            "Remove Worktree?",
            format!(
                "Remove {path}? Its branch will be kept. Removal is refused if it contains uncommitted, untracked, or ignored files."
            ),
            "Remove Worktree",
            on_confirm,
        )
    }

    /// The reminder shown when a check is requested while an installed update
    /// waits for a restart.
    pub fn restart_required(release: Option<&ReleaseInfo>, on_confirm: impl FnOnce(&mut Window, &mut App) + 'static) -> Self {
        let mut confirm = Self::new(
            "Restart Required",
            "The update is ready. Please restart the application to complete the installation.",
            "Restart Now",
            on_confirm,
        );
        confirm.cancel_label = Some("Restart Later".to_string());
        if let Some(release) = release {
            confirm.details.push(("Version:".to_string(), release.display_version().to_string()));
            if let Some(date) = release.published_at {
                confirm.details.push(("Released:".to_string(), abbreviated_date(civil(date))));
            }
        }
        confirm
    }

    /// The "Change Tab Title" prompt: the text is passed to `on_confirm`.
    pub fn prompt_title(initial: &str, on_confirm: impl FnOnce(String, &mut Window, &mut App) + 'static) -> Self {
        let mut confirm = Self::new("Change Tab Title", "Leave blank to restore the default.", "OK", |_, _| {});
        confirm.input = Some(initial.to_string());
        confirm.on_confirm = Box::new(move |text, window, cx| on_confirm(text.unwrap_or_default(), window, cx));
        confirm
    }
}

pub struct Dismissed;

impl EventEmitter<Dismissed> for ConfirmView {}

pub struct ConfirmView {
    request: Option<Confirm>,
    input: Option<Entity<InputState>>,
    focus: FocusHandle,
    _subscription: Option<Subscription>,
}

impl ConfirmView {
    pub fn new(request: Confirm, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = request.input.as_ref().map(|initial| ui::new_input(window, cx, "", initial));
        let subscription = input.as_ref().map(|input| {
            cx.subscribe_in(input, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.finish(true, window, cx);
                }
            })
        });
        if let Some(input) = &input {
            input.update(cx, |state, cx| state.focus(window, cx));
        } else {
            window.focus(&cx.focus_handle(), cx);
        }
        Self { request: Some(request), input, focus: cx.focus_handle(), _subscription: subscription }
    }

    fn finish(&mut self, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = self.request.take() else { return };
        let value = self.input.as_ref().map(|i| i.read(cx).value().to_string());
        cx.emit(Dismissed);
        if confirmed {
            (request.on_confirm)(value, window, cx);
        }
    }
}

impl Render for ConfirmView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let Some(request) = &self.request else { return div().into_any_element() };
        let mut body = div().v_flex().gap(px(8.)).flex_1().min_w_0().child(
            div().text_size(px(text::HEADLINE)).font_weight(FontWeight::BOLD).child(request.title.clone()),
        );
        if let Some(message) = &request.message {
            body = body.child(div().text_size(px(text::SUBHEADLINE)).child(message.clone()));
        }
        if !request.details.is_empty() {
            body = body.child(div().v_flex().gap(px(4.)).children(request.details.iter().map(|(label, value)| {
                div()
                    .h_flex()
                    .gap(px(6.))
                    .text_size(px(text::SUBHEADLINE))
                    .child(div().w(px(60.)).flex_none().text_right().text_color(look.secondary).child(label.clone()))
                    .child(value.clone())
            })));
        }
        if let Some(input) = &self.input {
            body = body.child(ui::field(input));
        }

        let mut buttons = div().h_flex().justify_end().gap(px(8.));
        if let Some(cancel) = &request.cancel_label {
            buttons = buttons.child(
                ui::button("confirm-cancel", cancel.clone(), &look, Variant::Regular, false, true)
                    .on_click(cx.listener(|this, _, window, cx| this.finish(false, window, cx))),
            );
        }
        buttons = buttons.child(
            ui::button("confirm-ok", request.confirm_label.clone(), &look, Variant::Prominent, false, true)
                .on_click(cx.listener(|this, _, window, cx| this.finish(true, window, cx))),
        );

        let icon_image = app_icon("official").map(|image| img(image).size(px(64.)).flex_none());
        let has_cancel = request.cancel_label.is_some();
        let _ = window;
        sheet(&look, 420.)
            .id("confirm-dialog")
            .key_context("ConfirmDialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "escape" if has_cancel => this.finish(false, window, cx),
                    "escape" => this.finish(true, window, cx),
                    "enter" => this.finish(true, window, cx),
                    _ => {}
                }
            }))
            .p(px(20.))
            .v_flex()
            .gap(px(16.))
            .child(div().h_flex().gap(px(16.)).items_start().children(icon_image).child(body))
            .child(buttons)
            .into_any_element()
    }
}

/// A card hanging from the top of the window, like an AppKit sheet.
pub fn sheet(look: &Look, width: f32) -> Div {
    let mut card = ui::card(look, 12.).occlude().w(px(width)).text_color(look.primary).font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT));
    card.style().corner_radii.top_left = Some(px(0.).into());
    card.style().corner_radii.top_right = Some(px(0.).into());
    card.border_t_0()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardKind {
    Paste,
    Osc52Read,
    Osc52Write,
    KittyRead,
    KittyWrite,
}

impl ClipboardKind {
    /// The explanation shown in the prompt. The Kitty protocol's own name for
    /// the request is attacker-controlled and deliberately never shown.
    pub fn message(self) -> &'static str {
        match self {
            Self::Paste => {
                "Pasting this text to the terminal may be dangerous as it looks like some commands may be executed."
            }
            Self::Osc52Read | Self::KittyRead => {
                "An application is attempting to read from the clipboard.\nThe current clipboard contents are shown below."
            }
            Self::Osc52Write | Self::KittyWrite => {
                "An application is attempting to write to the clipboard.\nThe content to write is shown below."
            }
        }
    }

    pub fn cancel_label(self) -> &'static str {
        match self {
            Self::Paste => "Cancel",
            _ => "Deny",
        }
    }

    pub fn confirm_label(self) -> &'static str {
        match self {
            Self::Paste => "Paste",
            _ => "Allow",
        }
    }
}

/// An unsafe paste or an OSC 52 / Kitty clipboard access awaiting a decision.
/// `completion` runs once with `(confirmed, remember)`; dropping the request
/// unanswered counts as a denial.
pub struct ClipboardRequest {
    pub kind: ClipboardKind,
    pub contents: String,
    /// Offer "Remember this choice for the session".
    pub can_remember: bool,
    pub image: Option<Arc<Image>>,
    pub completion: Box<dyn FnOnce(bool, bool)>,
}

pub struct ClipboardView {
    request: Option<ClipboardRequest>,
    remember: bool,
    focus: FocusHandle,
}

impl EventEmitter<Dismissed> for ClipboardView {}

impl ClipboardView {
    pub fn new(request: ClipboardRequest, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self { request: Some(request), remember: false, focus }
    }

    fn finish(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        let Some(request) = self.request.take() else { return };
        cx.emit(Dismissed);
        (request.completion)(confirmed, confirmed && self.remember);
    }
}

impl Drop for ClipboardView {
    fn drop(&mut self) {
        if let Some(request) = self.request.take() {
            (request.completion)(false, false);
        }
    }
}

impl Render for ClipboardView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let Some(request) = &self.request else { return div().into_any_element() };
        let kind = request.kind;

        let preview = match &request.image {
            Some(image) => div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .px(px(16.))
                .child(img(image.clone()).max_h_full().max_w_full())
                .into_any_element(),
            None => div()
                .id("clipboard-contents")
                .flex_1()
                .min_h_0()
                .mx(px(16.))
                .p(px(6.))
                .rounded(px(5.))
                .bg(look.field)
                .border_1()
                .border_color(look.separator)
                .overflow_y_scroll()
                .font_family("Menlo")
                .text_size(px(text::BODY))
                .whitespace_normal()
                .child(request.contents.clone())
                .into_any_element(),
        };

        let remember = self.remember;
        let remember_row = request.can_remember.then(|| {
            div()
                .id("clipboard-remember")
                .h_flex()
                .gap(px(6.))
                .items_center()
                .justify_center()
                .pt(px(4.))
                .cursor_pointer()
                .text_size(px(text::BODY))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.remember = !this.remember;
                    cx.notify();
                }))
                .child(
                    div()
                        .size(px(14.))
                        .rounded(px(3.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(remember, |d| d.bg(look.accent))
                        .when(!remember, |d| d.border_1().border_color(look.primary.opacity(0.4)))
                        .when(remember, |d| d.child(icon(IconName::Check, 10., white()))),
                )
                .child("Remember this choice for the session")
        });

        sheet(&look, 480.)
            .id("clipboard-confirm")
            .key_context("ClipboardConfirm")
            .track_focus(&self.focus)
            .h(px(270.))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| match event.keystroke.key.as_str() {
                "escape" => this.finish(false, cx),
                "enter" => this.finish(true, cx),
                _ => {}
            }))
            .v_flex()
            .gap(px(8.))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .child(div().p(px(16.)).child(icon(IconName::TriangleAlert, 42., look.yellow)))
                    .child(div().flex_1().min_w_0().p(px(16.)).text_size(px(text::BODY)).child(kind.message())),
            )
            .child(preview)
            .children(remember_row)
            .child(
                div()
                    .h_flex()
                    .justify_center()
                    .gap(px(8.))
                    .pb(px(16.))
                    .child(
                        ui::button("clipboard-cancel", kind.cancel_label(), &look, Variant::Regular, false, true)
                            .on_click(cx.listener(|this, _, _, cx| this.finish(false, cx))),
                    )
                    .child(
                        ui::button("clipboard-confirm-btn", kind.confirm_label(), &look, Variant::Prominent, false, true)
                            .on_click(cx.listener(|this, _, _, cx| this.finish(true, cx))),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn labels_depend_on_the_request_kind() {
        assert_eq!(ClipboardKind::Paste.cancel_label(), "Cancel");
        assert_eq!(ClipboardKind::Paste.confirm_label(), "Paste");
        for kind in [ClipboardKind::Osc52Read, ClipboardKind::Osc52Write, ClipboardKind::KittyRead, ClipboardKind::KittyWrite] {
            assert_eq!(kind.cancel_label(), "Deny");
            assert_eq!(kind.confirm_label(), "Allow");
        }
    }

    #[test]
    fn read_and_write_requests_explain_their_direction() {
        assert!(ClipboardKind::Osc52Read.message().contains("read from the clipboard"));
        assert!(ClipboardKind::KittyWrite.message().contains("write to the clipboard"));
        assert_eq!(ClipboardKind::KittyRead.message(), ClipboardKind::Osc52Read.message());
    }

    #[test]
    fn restart_notice_lists_the_release() {
        let release = ReleaseInfo {
            tag: "v2.0.0".into(),
            name: None,
            page_url: String::new(),
            published_at: None,
            body: None,
            package: None,
            checksum: None,
        };
        let confirm = Confirm::restart_required(Some(&release), |_, _| {});
        assert_eq!(confirm.confirm_label, "Restart Now");
        assert_eq!(confirm.cancel_label.as_deref(), Some("Restart Later"));
        assert_eq!(confirm.details, [("Version:".to_string(), "2.0.0".to_string())]);
    }

    #[test]
    fn notices_have_no_cancel_button() {
        assert_eq!(Confirm::notice("Oops", "Something").cancel_label, None);
    }
}
