//! The AI quota footer at the bottom of the sidebar: one usage ring per
//! provider, a popover with the details, and the settings gear.

use std::time::{Duration, Instant, SystemTime};

use angelly_core::quota::{Provider, QuotaError, QuotaSnapshot, QuotaState, QuotaStore, QuotaWindow};
use gpui_kit::assets::IconName;
use gpui_kit::base::Button;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::assets::{claude_color, codex_black, codex_white};
use super::format::{abbreviated_date_time, civil, relative_duration, short_time};
use super::ui::{self, icon, text, Look, Variant};
use super::OpenSettings;
use crate::prefs::Prefs;

/// Shared quota state, refreshed on worker threads.
pub struct QuotaModel {
    store: QuotaStore,
    observer: u64,
}

impl QuotaModel {
    pub fn new(store: QuotaStore, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<()>();
        store.subscribe(move || {
            let _ = tx.try_send(());
        });
        cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                while rx.try_recv().is_ok() {}
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { store, observer: 1 }
    }

    pub fn state(&self, provider: Provider) -> QuotaState {
        self.store.state(provider)
    }

    pub fn observe(&self) {
        self.store.observe(self.observer);
    }

    pub fn stop_observing(&self) {
        self.store.stop_observing(self.observer);
    }

    pub fn refresh(&self, provider: Provider, allow_interaction: bool, force: bool) {
        self.store.refresh(provider, allow_interaction, force);
    }
}

pub fn percent_text(remaining: f64) -> String {
    format!("{remaining:.0}%")
}

/// The one-line status shown in a provider button's tooltip.
pub fn ring_detail(state: &QuotaState, provider: Provider) -> String {
    if state.is_loading {
        return "Loading quota".to_string();
    }
    if let Some(window) = state.snapshot.as_ref().and_then(QuotaSnapshot::ring_window) {
        return format!("{}: {} remaining", window.kind.label(), percent_text(window.remaining_percent));
    }
    state.error.map(|e| e.message(provider)).unwrap_or_else(|| "Quota not loaded".to_string())
}

pub fn reset_text(window: &QuotaWindow) -> String {
    match window.resets_at {
        Some(at) => format!("Resets {}", abbreviated_date_time(civil(at))),
        None => "Reset time unavailable".to_string(),
    }
}

pub fn last_updated_text(at: SystemTime) -> String {
    format!("Last updated {}", short_time(civil(at)))
}

/// Seconds until a rate-limit cooldown ends, if it hasn't.
pub fn cooldown_remaining(state: &QuotaState, now: SystemTime) -> Option<Duration> {
    state.retry_after()?.duration_since(now).ok().filter(|d| !d.is_zero())
}

fn logo(provider: Provider, dark: bool) -> std::sync::Arc<Image> {
    match (provider, dark) {
        (Provider::Codex, true) => codex_white(),
        (Provider::Codex, false) => codex_black(),
        (Provider::Claude, _) => claude_color(),
    }
}

fn provider_id(provider: Provider) -> &'static str {
    match provider {
        Provider::Codex => "codex",
        Provider::Claude => "claude",
    }
}

/// The footer view. One per window; its popovers hang off its buttons.
pub struct QuotaFooter {
    model: Entity<QuotaModel>,
    open: Option<Provider>,
    dismissed: Option<(Provider, Instant)>,
    last_render: Instant,
    focus: FocusHandle,
    ticker: Option<Task<()>>,
}

impl QuotaFooter {
    pub fn new(model: Entity<QuotaModel>, cx: &mut Context<Self>) -> Self {
        let this = Self {
            model,
            open: None,
            dismissed: None,
            last_render: Instant::now(),
            focus: cx.focus_handle(),
            ticker: None,
        };
        this.watch_visibility(cx);
        this
    }

    /// The store polls only while the footer is on screen: stop observing when
    /// it hasn't been drawn for a while.
    fn watch_visibility(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let alive = this.update(cx, |this, cx| {
                if this.last_render.elapsed() > Duration::from_secs(2) {
                    this.model.read(cx).stop_observing();
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    pub fn open_popover(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        self.open = Some(provider);
        self.model.read(cx).refresh(provider, false, false);
        window.focus(&self.focus, cx);
        self.ticker = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }));
        cx.notify();
    }

    pub fn close_popover(&mut self, cx: &mut Context<Self>) {
        self.open = None;
        self.ticker = None;
        cx.notify();
    }

    fn toggle(&mut self, provider: Provider, window: &mut Window, cx: &mut Context<Self>) {
        if self.dismissed.is_some_and(|(p, at)| p == provider && at.elapsed() < Duration::from_millis(300)) {
            return;
        }
        if self.open == Some(provider) {
            self.close_popover(cx);
        } else {
            self.open_popover(provider, window, cx);
        }
    }

    fn provider_button(&self, provider: Provider, look: &Look, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.read(cx).state(provider);
        let fraction = state
            .snapshot
            .as_ref()
            .and_then(QuotaSnapshot::ring_window)
            .map(|w| w.remaining_percent / 100.0);
        let tip = format!("{}\n{}", provider.name(), ring_detail(&state, provider));
        let hover = look.primary.opacity(0.08);
        let focus = look.accent;
        let open = self.open == Some(provider);
        Button::new(ui::named("quota", provider_id(provider)))
            .accessibility_label(tip.replace('\n', ", "))
            .aria_expanded(open)
            .relative()
            .size(px(36.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .focus_visible(move |s| s.border_1().border_color(focus))
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, window, cx| this.toggle(provider, window, cx)))
            .child(
                div()
                    .size(px(28.))
                    .relative()
                    .child(ui::ring(28., 2., fraction, look.primary.opacity(0.15), look.primary))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(img(logo(provider, look.dark)).size(px(18.))),
                    ),
            )
            .when(open, |d| {
                d.child(div().absolute().top_0().left_0().child(deferred(
                    anchored()
                        .anchor(Anchor::BottomLeft)
                        .offset(point(px(44.), px(36.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .relative()
                                .child(self.popover(provider, &state, look, cx))
                                .child(div().absolute().left(px(-6.)).bottom(px(8.)).child(ui::arrow_left(look))),
                        ),
                )))
            })
    }

    fn quota_window(window: &QuotaWindow, look: &Look) -> Div {
        div()
            .v_flex()
            .gap(px(5.))
            .child(
                div()
                    .h_flex()
                    .justify_between()
                    .text_size(px(text::BODY))
                    .child(window.kind.label())
                    .child(format!("{} remaining", percent_text(window.remaining_percent))),
            )
            .child(ui::progress_bar(window.remaining_percent / 100.0, look))
            .child(div().text_size(px(text::CAPTION)).text_color(look.primary).child(reset_text(window)))
    }

    fn popover(&self, provider: Provider, state: &QuotaState, look: &Look, cx: &mut Context<Self>) -> impl IntoElement {
        let now = SystemTime::now();
        let cooldown = cooldown_remaining(state, now);
        let caption = |text: String| div().text_size(px(text::CAPTION)).text_color(look.primary).child(text);

        let mut body = div().v_flex().gap(px(14.)).child(
            div().text_size(px(text::HEADLINE)).font_weight(FontWeight::BOLD).child(provider.name()),
        );
        if state.is_loading {
            body = body.child(
                div()
                    .h_flex()
                    .gap(px(8.))
                    .items_center()
                    .child(ui::spinner("quota-spin", 14., look.primary, 1.0))
                    .child(div().text_size(px(text::BODY)).child("Loading quota…")),
            );
        } else if let Some(snapshot) = &state.snapshot {
            if let Some(window) = &snapshot.five_hour {
                body = body.child(Self::quota_window(window, look));
            }
            if let Some(window) = &snapshot.weekly {
                body = body.child(Self::quota_window(window, look));
            }
            if snapshot.five_hour.is_none() {
                body = body.child(caption("The ring shows weekly quota because no 5-hour quota was reported.".into()));
            }
        } else {
            let message = state.error.unwrap_or(QuotaError::Unavailable).message(provider);
            body = body.child(div().text_size(px(text::BODY)).child(message));
        }
        if let Some(updated) = state.last_updated {
            body = body.child(caption(last_updated_text(updated)));
        }
        if let Some(wait) = cooldown {
            body = body.child(caption(format!("Refresh available {}", relative_duration(wait.as_secs()))));
        }

        let model = self.model.clone();
        let allow_model = self.model.clone();
        let refresh_enabled = !state.is_loading && cooldown.is_none();
        let needs_permission = state.error == Some(QuotaError::PermissionRequired);
        body = body.child(
            div()
                .h_flex()
                .items_center()
                .justify_between()
                .child(div().when(needs_permission, |d| {
                    d.child(
                        ui::button("quota-allow", "Allow Keychain Access…", look, Variant::Regular, false, true)
                            .on_click(move |_, _, cx| allow_model.read(cx).refresh(provider, true, true)),
                    )
                }))
                .child(ui::button("quota-refresh", "Refresh", look, Variant::Prominent, false, refresh_enabled).when(
                    refresh_enabled,
                    |b| b.on_click(move |_, _, cx| model.read(cx).refresh(provider, false, true)),
                )),
        );

        ui::card(look, 10.)
            .id(ui::named("quota-popover", provider_id(provider)))
            .track_focus(&self.focus)
            .key_context("QuotaPopover")
            .occlude()
            .w(px(300.))
            .p(px(16.))
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .on_mouse_down_out(cx.listener(move |this, _, _, cx| {
                this.dismissed = Some((provider, Instant::now()));
                this.close_popover(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| match event.keystroke.key.as_str() {
                "escape" => this.close_popover(cx),
                _ => {}
            }))
            .child(body)
    }
}

impl Render for QuotaFooter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.last_render = Instant::now();
        let look = Look::of(cx);
        let shows_quota = Prefs::read(cx).shows_quota();
        if shows_quota {
            self.model.read(cx).observe();
        } else {
            self.model.read(cx).stop_observing();
        }
        let hover = look.primary.opacity(0.08);
        let focus = look.accent;
        div()
            .v_flex()
            .flex_none()
            .w_full()
            .child(ui::divider(&look))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(10.))
                    .py(px(6.))
                    .when(shows_quota, |row| {
                        row.child(self.provider_button(Provider::Codex, &look, cx))
                            .child(self.provider_button(Provider::Claude, &look, cx))
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("sidebar-settings")
                            .accessibility_label("Settings")
                            .size(px(36.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .focus_visible(move |s| s.border_1().border_color(focus))
                            .tooltip(|window, cx| Tooltip::new("Settings").build(window, cx))
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenSettings), cx))
                            .child(icon(IconName::Settings, 20., look.primary.opacity(0.85))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;
    use angelly_core::quota::WindowKind;
    use std::time::UNIX_EPOCH;

    fn window(kind: WindowKind, remaining: f64) -> QuotaWindow {
        QuotaWindow { kind, remaining_percent: remaining, resets_at: None }
    }

    fn state(snapshot: Option<QuotaSnapshot>, error: Option<QuotaError>, loading: bool) -> QuotaState {
        QuotaState { snapshot, error, last_updated: None, is_loading: loading }
    }

    #[test]
    fn percentages_round_to_whole_numbers() {
        assert_eq!(percent_text(73.4), "73%");
        assert_eq!(percent_text(0.0), "0%");
        assert_eq!(percent_text(99.6), "100%");
    }

    #[test]
    fn tooltip_detail_prefers_the_ring_window() {
        let both = QuotaSnapshot {
            five_hour: Some(window(WindowKind::FiveHour, 42.0)),
            weekly: Some(window(WindowKind::Weekly, 80.0)),
        };
        assert_eq!(ring_detail(&state(Some(both), None, false), Provider::Codex), "5-hour: 42% remaining");
        let weekly = QuotaSnapshot { five_hour: None, weekly: Some(window(WindowKind::Weekly, 80.0)) };
        assert_eq!(ring_detail(&state(Some(weekly), None, false), Provider::Claude), "Weekly: 80% remaining");
    }

    #[test]
    fn tooltip_detail_covers_loading_error_and_empty() {
        assert_eq!(ring_detail(&state(None, None, true), Provider::Codex), "Loading quota");
        assert_eq!(ring_detail(&state(None, None, false), Provider::Codex), "Quota not loaded");
        assert_eq!(
            ring_detail(&state(None, Some(QuotaError::NotSignedIn), false), Provider::Codex),
            "No local login found. Run codex login in your terminal, then refresh."
        );
    }

    #[test]
    fn reset_text_handles_missing_dates() {
        assert_eq!(reset_text(&window(WindowKind::Weekly, 1.0)), "Reset time unavailable");
        let with = QuotaWindow { resets_at: Some(UNIX_EPOCH + Duration::from_secs(86_400 * 365)), ..window(WindowKind::Weekly, 1.0) };
        assert!(reset_text(&with).starts_with("Resets "));
    }

    #[test]
    fn cooldown_only_counts_future_rate_limits() {
        let now = UNIX_EPOCH + Duration::from_secs(1000);
        let limited = state(None, Some(QuotaError::RateLimited { until: now + Duration::from_secs(30) }), false);
        assert_eq!(cooldown_remaining(&limited, now), Some(Duration::from_secs(30)));
        let expired = state(None, Some(QuotaError::RateLimited { until: now - Duration::from_secs(1) }), false);
        assert_eq!(cooldown_remaining(&expired, now), None);
        assert_eq!(cooldown_remaining(&state(None, None, false), now), None);
    }
}
