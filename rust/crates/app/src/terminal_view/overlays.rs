//! Transient and status overlays drawn above the terminal grid: the find
//! bar, resize indicator, progress bar, bell border, link preview, read-only
//! badge and the pending key sequence.

use std::time::Instant;

use angelly_core::persist::UntrustedUrl;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input};
use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::*;
use terminal::ProgressState;

use super::settings::{OverlayPosition, ResizeOverlay};
use super::TerminalView;
use crate::keybinds::SearchDirection;
use crate::theme::Theme;

const EDGE: f32 = 12.0;
const BELL_BORDER: f32 = 2.0;
const PROGRESS_HEIGHT: f32 = 2.0;
const SEARCH_WIDTH: f32 = 220.0;

fn badge(theme: &Theme) -> Div {
    div()
        .px(px(10.))
        .py(px(5.))
        .rounded(px(8.))
        .bg(theme.background().opacity(0.92))
        .border_1()
        .border_color(theme.tint(0.18))
        .text_color(theme.primary)
        .text_size(px(12.))
}

fn place(d: Div, position: OverlayPosition) -> Div {
    let d = d.absolute();
    match position {
        OverlayPosition::Center => d.top_0().bottom_0().left_0().right_0().flex().items_center().justify_center(),
        OverlayPosition::TopLeft => d.top(px(EDGE)).left(px(EDGE)),
        OverlayPosition::TopCenter => d.top(px(EDGE)).left_0().right_0().flex().justify_center(),
        OverlayPosition::TopRight => d.top(px(EDGE)).right(px(EDGE)),
        OverlayPosition::BottomLeft => d.bottom(px(EDGE)).left(px(EDGE)),
        OverlayPosition::BottomCenter => d.bottom(px(EDGE)).left_0().right_0().flex().justify_center(),
        OverlayPosition::BottomRight => d.bottom(px(EDGE)).right(px(EDGE)),
    }
}

impl TerminalView {
    pub(crate) fn show_resize_toast(&mut self, cols: u16, rows: u16, cx: &mut Context<Self>) {
        if self.settings.resize_overlay == ResizeOverlay::Never {
            return;
        }
        let duration = self.settings.resize_overlay_duration;
        self.toast = Some(super::ResizeToast { size: (cols, rows), until: Instant::now() + duration });
        self.tasks.toast = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(duration).await;
            this.update(cx, |this, cx| {
                if this.toast.is_some_and(|t| t.until <= Instant::now()) {
                    this.toast = None;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    pub(crate) fn render_overlays(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let theme = cx.global::<Theme>();
        let mut out: Vec<AnyElement> = Vec::new();

        if let Some(bar) = self.progress_bar(theme) {
            out.push(bar.into_any_element());
        }
        if self.bell && self.settings.bell.border {
            out.push(div().absolute().inset_0().border(px(BELL_BORDER)).border_color(rgb(0xF5C542)).into_any_element());
        }
        if let Some(toast) = self.toast.filter(|t| t.until > Instant::now()) {
            let text = format!("{} \u{00D7} {}", toast.size.0, toast.size.1);
            out.push(
                place(div(), self.settings.resize_overlay_position).child(badge(theme).child(text)).into_any_element(),
            );
        }
        if self.readonly {
            out.push(
                div()
                    .absolute()
                    .top(px(EDGE))
                    .right(px(EDGE))
                    .child(badge(theme).child("Read-only"))
                    .into_any_element(),
            );
        }
        if let Some(sequence) = self.key_sequence_label() {
            out.push(
                div()
                    .absolute()
                    .bottom(px(EDGE))
                    .right(px(EDGE))
                    .child(badge(theme).child(sequence))
                    .into_any_element(),
            );
        }
        if let Some(link) = self.hover_link.as_ref().filter(|l| self.settings.link_previews.shows(l.osc8)) {
            let text = UntrustedUrl::new(link.text()).display_string();
            out.push(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .max_w(relative(0.6))
                    .px(px(6.))
                    .py(px(3.))
                    .rounded_tr(px(8.))
                    .bg(theme.background().opacity(0.95))
                    .text_color(theme.primary)
                    .text_size(px(12.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(text)
                    .into_any_element(),
            );
        }
        if self.search.is_some() {
            out.push(self.search_bar(cx).into_any_element());
        }
        out
    }

    fn progress_bar(&self, theme: &Theme) -> Option<Div> {
        let progress = self.progress?;
        let color = match progress.state {
            ProgressState::Error => rgb(0xFF453A),
            ProgressState::Pause => rgb(0xFF9F0A),
            _ => theme.accent.into(),
        };
        let track = div().absolute().top_0().left_0().right_0().h(px(PROGRESS_HEIGHT));
        let fill = match (progress.state, progress.percent) {
            (ProgressState::Indeterminate, _) => {
                let phase = (self.created.elapsed().as_secs_f32() * 0.8).fract();
                let width = 0.3;
                let left = phase * (1.0 + width) - width;
                let (visible_left, visible_right) = (left.max(0.0), (left + width).min(1.0));
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(visible_left))
                    .w(relative((visible_right - visible_left).max(0.0)))
                    .bg(color)
            }
            (_, percent) => {
                let fraction = f32::from(percent.unwrap_or(100)) / 100.0;
                div().absolute().top_0().bottom_0().left_0().w(relative(fraction.clamp(0.0, 1.0))).bg(color)
            }
        };
        Some(track.child(fill))
    }

    /// The active key tables and the keys of an unfinished sequence.
    fn key_sequence_label(&self) -> Option<String> {
        if self.keys.is_idle() {
            return None;
        }
        let mut parts: Vec<String> = self.keys.active_tables().map(str::to_string).collect();
        let keys: Vec<String> = self
            .keys
            .pending_chords()
            .map(|c| {
                let m = c.mods;
                let mut s = String::new();
                for (on, glyph) in
                    [(m.ctrl, '\u{2303}'), (m.alt, '\u{2325}'), (m.shift, '\u{21E7}'), (m.super_key, '\u{2318}')]
                {
                    if on {
                        s.push(glyph);
                    }
                }
                s.push_str(&c.base);
                s
            })
            .collect();
        if !keys.is_empty() {
            parts.push(keys.join(" "));
        }
        Some(parts.join(" \u{203A} "))
    }

    fn search_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<Theme>();
        let search = self.search.as_ref().expect("search bar is open");
        let empty = search.input.read(cx).value().is_empty();
        let count = match (empty, search.selected) {
            (true, _) => String::new(),
            (false, Some(i)) => format!("{}/{}", i + 1, search.total),
            (false, None) => format!("-/{}", search.total),
        };
        let nav = |id: &'static str, icon: IconName, direction: SearchDirection, cx: &mut Context<Self>| {
            Button::new(id).ghost().xsmall().icon(Icon::new(icon)).on_click(cx.listener(move |this, _, _, cx| {
                this.navigate_search(direction, cx);
            }))
        };
        div()
            .absolute()
            .top(px(EDGE))
            .right(px(EDGE))
            .occlude()
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                this.end_search(window, cx);
            }))
            .child(
                badge(theme)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(SEARCH_WIDTH))
                            .child(Input::new(&search.input).appearance(false).bordered(false).focus_bordered(false)),
                    )
                    .child(div().min_w(px(36.)).text_color(theme.secondary()).child(count))
                    .child(nav("find-prev", IconName::ChevronUp, SearchDirection::Next, cx))
                    .child(nav("find-next", IconName::ChevronDown, SearchDirection::Previous, cx))
                    .child(Button::new("find-close").ghost().xsmall().icon(Icon::new(IconName::Close)).on_click(
                        cx.listener(|this, _, window, cx| {
                            this.end_search(window, cx);
                        }),
                    )),
            )
    }
}
