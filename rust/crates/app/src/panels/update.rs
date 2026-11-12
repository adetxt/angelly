//! The update pill and its popover, driven by `UpdateController` state.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use angelly_core::update::{CheckStart, ReleaseInfo, Tone, UpdateController, UpdateIcon, UpdateState};
use gpui_kit::assets::IconName;
use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::format::{abbreviated_date, civil, file_size};
use super::ui::{self, icon, Look, Variant};

/// Installs a downloaded update. The platform layer implements this: unpack the
/// package, swap the bundle, report progress through the controller, and
/// relaunch.
pub trait UpdateInstaller: Send + Sync {
    /// Starts installing `package`, reporting `report_extraction` then
    /// `report_installing` on the controller.
    fn install(&self, package: PathBuf, controller: UpdateController);

    /// Quits and relaunches into the installed update.
    fn relaunch(&self);
}

/// Placeholder until the platform layer provides an installer: hands over to
/// the "Installing…" state and stops there.
pub struct UnimplementedInstaller;

impl UpdateInstaller for UnimplementedInstaller {
    fn install(&self, _: PathBuf, controller: UpdateController) {
        controller.report_installing(None);
    }

    fn relaunch(&self) {}
}

/// Raised by the model for things only a window can present.
pub enum UpdateEvent {
    /// A check was requested while an installed update awaits a restart.
    RestartRequired(Option<ReleaseInfo>),
}

impl EventEmitter<UpdateEvent> for UpdateModel {}

pub struct UpdateModel {
    pub controller: UpdateController,
    installer: Arc<dyn UpdateInstaller>,
    handed_over: Option<PathBuf>,
    dismiss: Option<Task<()>>,
    /// The running check was started in the background: it only shows up when it finds an update.
    silent: bool,
}

/// How often the app looks at whether an automatic check is due.
const AUTOMATIC_POLL: Duration = Duration::from_secs(60 * 60);
/// Delay before the first automatic check, so it does not compete with the launch.
const AUTOMATIC_FIRST_CHECK: Duration = Duration::from_secs(10);

/// Whether a background check should run: the `auto-update` setting allows it, the user did not
/// decline the permission prompt and the last check is at least a day old.
pub fn automatic_check_wanted(
    mode: Option<angelly_core::config::AutoUpdate>,
    answered: Option<bool>,
    last_check: Option<f64>,
    now: std::time::SystemTime,
) -> bool {
    let last = last_check.filter(|seconds| *seconds >= 0.0).map(|s| std::time::UNIX_EPOCH + Duration::from_secs_f64(s));
    answered != Some(false) && angelly_core::update::automatic_check_due(mode, last, now)
}

impl UpdateModel {
    pub fn new(controller: UpdateController, installer: Arc<dyn UpdateInstaller>, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<()>();
        controller.subscribe(move || {
            let _ = tx.try_send(());
        });
        cx.spawn(async move |this, cx| {
            while rx.recv().await.is_ok() {
                while rx.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.changed(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self { controller, installer, handed_over: None, dismiss: None, silent: false }
    }

    /// The state the pill shows: a background check stays invisible until it finds something.
    pub fn state(&self) -> UpdateState {
        match self.controller.state() {
            UpdateState::Checking if self.silent => UpdateState::Idle,
            state => state,
        }
    }

    /// Checks once shortly after launch and then whenever a day has passed since the last check.
    /// Self tests and runs without a data directory never check on their own.
    pub fn start_automatic_checks(&self, cx: &mut Context<Self>) {
        if crate::session::data_directory().is_none() {
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOMATIC_FIRST_CHECK).await;
            loop {
                if this.update(cx, |this, cx| this.automatic_check(cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(AUTOMATIC_POLL).await;
            }
        })
        .detach();
    }

    fn automatic_check(&mut self, cx: &mut Context<Self>) {
        let mode = cx.global::<crate::theme::Theme>().config.auto_update();
        let prefs = crate::prefs::Prefs::read(cx).preferences();
        let wanted = automatic_check_wanted(mode, prefs.update_automatic_checks(), prefs.update_last_check(), std::time::SystemTime::now());
        if !wanted || self.controller.state() != UpdateState::Idle {
            return;
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        crate::prefs::Prefs::update(cx, |prefs| prefs.set_update_last_check(now));
        self.silent = true;
        self.controller.check_for_updates();
    }

    pub fn set_installer(&mut self, installer: Arc<dyn UpdateInstaller>) {
        self.installer = installer;
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        let state = self.controller.state();
        self.dismiss = None;
        if self.silent && !matches!(state, UpdateState::Checking) {
            self.silent = false;
            if matches!(state, UpdateState::NotFound | UpdateState::Error { .. }) {
                self.controller.cancel();
                return;
            }
        }
        match &state {
            UpdateState::NotFound => {
                self.dismiss = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Duration::from_secs(5)).await;
                    let _ = this.update(cx, |this, _| {
                        if this.controller.state() == UpdateState::NotFound {
                            this.controller.cancel();
                        }
                    });
                }));
            }
            UpdateState::Extracting { .. } => {
                if let Some(package) = self.controller.downloaded_package().filter(|p| Some(p) != self.handed_over.as_ref()) {
                    self.handed_over = Some(package.clone());
                    let (installer, controller) = (self.installer.clone(), self.controller.clone());
                    std::thread::spawn(move || installer.install(package, controller));
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// Checks for updates, or asks the user to restart when one is installed.
    pub fn check(&mut self, cx: &mut Context<Self>) {
        self.silent = false;
        if let CheckStart::RestartRequired = self.controller.check_for_updates() {
            let release = match self.controller.state() {
                UpdateState::Installing { release } => release,
                _ => None,
            };
            cx.emit(UpdateEvent::RestartRequired(release));
        }
    }

    /// Accepts the current state; relaunches when the update is already installed.
    pub fn confirm(&mut self) {
        if self.controller.confirm() {
            self.installer.relaunch();
        }
    }

    pub fn cancel(&self) {
        self.controller.cancel();
    }

    pub fn retry(&self) {
        self.controller.retry();
    }

    pub fn answer_permission(&self) {
        self.controller.answer_permission();
    }
}

pub fn icon_of(icon: UpdateIcon) -> IconName {
    match icon {
        UpdateIcon::Question => IconName::CircleQuestionMark,
        UpdateIcon::Refresh => IconName::RefreshCw,
        UpdateIcon::Package => IconName::Package,
        UpdateIcon::PackageFilled => IconName::Package2,
        UpdateIcon::ArrowDown => IconName::CircleArrowDown,
        UpdateIcon::Power => IconName::Power,
        UpdateIcon::Info => IconName::Info,
        UpdateIcon::Warning => IconName::TriangleAlert,
    }
}

fn tone_color(tone: Tone, look: &Look) -> Hsla {
    let blue: Hsla = rgb(0x0A84FF).into();
    match tone {
        Tone::Primary => look.primary,
        Tone::Secondary => look.secondary,
        Tone::OnAccent => white(),
        Tone::Accent => look.accent,
        Tone::BlueDark => blue.blend(black().opacity(0.3)),
        Tone::BlueDarker => blue.blend(black().opacity(0.5)),
        Tone::Warning => look.orange,
        Tone::WarningTint => look.orange.opacity(0.2),
        Tone::Control => look.card,
    }
}

fn download_fraction(expected: Option<u64>, progress: u64) -> Option<f64> {
    expected.filter(|e| *e > 0).map(|e| (progress as f64 / e as f64).clamp(0.0, 1.0))
}

pub fn text_width(window: &Window, text: &str, size: f32) -> Pixels {
    let mut font = font(ui::FONT);
    font.weight = FontWeight::MEDIUM;
    let run = TextRun { len: text.len(), font, color: black(), background_color: None, underline: None, strikethrough: None };
    window.text_system().layout_line(text, px(size), &[run], None).width
}

/// The capsule button at the bottom right of the terminal area.
pub struct UpdatePill {
    model: Entity<UpdateModel>,
    open: bool,
    focus: FocusHandle,
}

impl UpdatePill {
    pub fn new(model: Entity<UpdateModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |this, model, cx| {
            if model.read(cx).state().is_hidden() {
                this.open = false;
            }
            cx.notify();
        })
        .detach();
        Self { model, open: false, focus: cx.focus_handle() }
    }

    pub fn open_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn badge(state: &UpdateState, look: &Look, fg: Hsla) -> AnyElement {
        match state {
            UpdateState::Downloading { expected_length, progress } => match download_fraction(*expected_length, *progress) {
                Some(fraction) => ui::ring(14., 2., Some(fraction), look.primary.opacity(0.2), look.primary).into_any_element(),
                None => icon(IconName::CircleArrowDown, 14., fg).into_any_element(),
            },
            UpdateState::Extracting { progress } => {
                ui::ring(14., 2., Some(*progress), look.primary.opacity(0.2), look.primary).into_any_element()
            }
            UpdateState::Checking => ui::spinner("update-checking", 14., fg, 2.5).into_any_element(),
            other => match other.icon() {
                Some(name) => icon(icon_of(name), 14., fg).into_any_element(),
                None => div().into_any_element(),
            },
        }
    }

    fn popover(&self, state: &UpdateState, look: &Look, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.clone();
        let content = popover_content(state, look, &model, cx.entity().downgrade());
        ui::card(look, 10.)
            .id("update-popover")
            .track_focus(&self.focus)
            .key_context("UpdatePopover")
            .occlude()
            .w(px(300.))
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.open = false;
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.open = false;
                    cx.notify();
                }
            }))
            .child(content)
    }
}

type Pill = WeakEntity<UpdatePill>;

fn dismiss(pill: &Pill, cx: &mut App) {
    let _ = pill.update(cx, |this, cx| {
        this.open = false;
        cx.notify();
    });
}

fn heading(label: &str) -> Div {
    div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).child(label.to_string())
}

fn info(look: &Look, label: &str) -> Div {
    div().text_size(px(11.)).text_color(look.secondary).child(label.to_string())
}

fn property(look: &Look, label: &str, value: String) -> Div {
    div()
        .h_flex()
        .gap(px(6.))
        .text_size(px(11.))
        .child(div().w(px(60.)).flex_none().text_color(look.secondary).text_right().child(label.to_string()))
        .child(value)
}

fn progress_block(look: &Look, fraction: f64) -> Div {
    div()
        .v_flex()
        .gap(px(6.))
        .child(ui::progress_bar(fraction, look))
        .child(info(look, &format!("{:.0}%", fraction * 100.0)))
}

fn popover_content(state: &UpdateState, look: &Look, model: &Entity<UpdateModel>, pill: Pill) -> Div {
    let padded = || div().v_flex().gap(px(16.)).p(px(16.));
    let small = |id: &'static str, label: &'static str, variant: Variant| ui::button(id, label, look, variant, true, true);
    match state {
        UpdateState::Idle | UpdateState::Installing { .. } => div(),
        UpdateState::PermissionRequest => {
            let (m1, m2, p1, p2) = (model.clone(), model.clone(), pill.clone(), pill);
            padded()
                .child(
                    div()
                        .v_flex()
                        .gap(px(8.))
                        .child(heading("Enable automatic updates?"))
                        .child(info(look, "Ghostty can automatically check for updates in the background.")),
                )
                .child(
                    div()
                        .h_flex()
                        .gap(px(8.))
                        .justify_between()
                        .child(ui::button("update-not-now", "Not Now", look, Variant::Regular, false, true).on_click(
                            move |_, _, cx| {
                                crate::prefs::Prefs::update(cx, |prefs| prefs.set_update_automatic_checks(false));
                                m1.read(cx).answer_permission();
                                dismiss(&p1, cx);
                            },
                        ))
                        .child(ui::button("update-allow", "Allow", look, Variant::Prominent, false, true).on_click(
                            move |_, _, cx| {
                                crate::prefs::Prefs::update(cx, |prefs| prefs.set_update_automatic_checks(true));
                                m2.read(cx).answer_permission();
                                dismiss(&p2, cx);
                            },
                        )),
                )
        }
        UpdateState::Checking => {
            let (m, p) = (model.clone(), pill);
            padded()
                .child(
                    div()
                        .h_flex()
                        .gap(px(10.))
                        .items_center()
                        .child(ui::spinner("update-pop-spin", 14., look.primary, 1.0))
                        .child(div().text_size(px(13.)).child("Checking for updates…")),
                )
                .child(div().h_flex().justify_end().child(small("update-cancel", "Cancel", Variant::Regular).on_click(
                    move |_, _, cx| {
                        m.read(cx).cancel();
                        dismiss(&p, cx);
                    },
                )))
        }
        UpdateState::UpdateAvailable(release) => {
            let (m_skip, m_later, m_install) = (model.clone(), model.clone(), model.clone());
            let (p_skip, p_later, p_install) = (pill.clone(), pill.clone(), pill);
            let mut rows = div()
                .v_flex()
                .gap(px(4.))
                .child(property(look, "Version:", release.display_version().to_string()));
            if let Some(size) = release.package.as_ref().and_then(|a| a.size).filter(|s| *s > 0) {
                rows = rows.child(property(look, "Size:", file_size(size)));
            }
            if let Some(date) = release.published_at {
                rows = rows.child(property(look, "Released:", abbreviated_date(civil(date))));
            }
            let body = div()
                .v_flex()
                .gap(px(12.))
                .p(px(16.))
                .child(div().v_flex().gap(px(8.)).child(heading("Update Available")).child(rows))
                .child(
                    div()
                        .h_flex()
                        .gap(px(8.))
                        .child(small("update-skip", "Skip", Variant::Regular).on_click(move |_, _, cx| {
                            m_skip.read(cx).cancel();
                            dismiss(&p_skip, cx);
                        }))
                        .child(small("update-later", "Later", Variant::Regular).on_click(move |_, _, cx| {
                            m_later.read(cx).cancel();
                            dismiss(&p_later, cx);
                        }))
                        .child(div().flex_1())
                        .child(small("update-install", "Install and Relaunch", Variant::Prominent).on_click(
                            move |_, _, cx| {
                                m_install.update(cx, |m, _| m.confirm());
                                dismiss(&p_install, cx);
                            },
                        )),
                );
            let mut column = div().v_flex().child(body);
            if let Some(notes) = release.notes() {
                let url = notes.url().to_string();
                let hover = look.primary.opacity(0.06);
                column = column.child(ui::divider(look)).child(
                    div()
                        .id("update-notes")
                        .h_flex()
                        .items_center()
                        .gap(px(6.))
                        .p(px(12.))
                        .w_full()
                        .bg(look.primary.opacity(0.04))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(move |_, _, cx| cx.open_url(&url))
                        .child(icon(IconName::FileText, 11., look.primary))
                        .child(div().text_size(px(11.)).font_weight(FontWeight::MEDIUM).child(notes.label()))
                        .child(div().flex_1())
                        .child(icon(IconName::ArrowUpRight, 10., look.primary)),
                );
            }
            column
        }
        UpdateState::Downloading { expected_length, progress } => {
            let (m, p) = (model.clone(), pill);
            let body = match download_fraction(*expected_length, *progress) {
                Some(fraction) => progress_block(look, fraction),
                None => div().child(ui::spinner("update-dl-spin", 14., look.primary, 1.0)),
            };
            padded()
                .child(div().v_flex().gap(px(8.)).child(heading("Downloading Update")).child(body))
                .child(div().h_flex().justify_end().child(small("update-cancel-dl", "Cancel", Variant::Regular).on_click(
                    move |_, _, cx| {
                        m.read(cx).cancel();
                        dismiss(&p, cx);
                    },
                )))
        }
        UpdateState::Extracting { progress } => div()
            .v_flex()
            .gap(px(8.))
            .p(px(16.))
            .child(heading("Preparing Update"))
            .child(progress_block(look, progress.clamp(0.0, 1.0))),
        UpdateState::NotFound => {
            let (m, p) = (model.clone(), pill);
            padded()
                .child(
                    div()
                        .v_flex()
                        .gap(px(8.))
                        .child(heading("No Updates Found"))
                        .child(info(look, "You're already running the latest version.")),
                )
                .child(div().h_flex().justify_end().child(small("update-ok", "OK", Variant::Prominent).on_click(
                    move |_, _, cx| {
                        m.read(cx).cancel();
                        dismiss(&p, cx);
                    },
                )))
        }
        UpdateState::Error { message } => {
            let (m_ok, m_retry) = (model.clone(), model.clone());
            let (p_ok, p_retry) = (pill.clone(), pill);
            padded()
                .child(
                    div()
                        .v_flex()
                        .gap(px(8.))
                        .child(
                            div()
                                .h_flex()
                                .gap(px(8.))
                                .items_center()
                                .child(icon(IconName::TriangleAlert, 13., look.orange))
                                .child(heading("Update Failed")),
                        )
                        .child(info(look, message)),
                )
                .child(
                    div()
                        .h_flex()
                        .justify_between()
                        .child(small("update-err-ok", "OK", Variant::Regular).on_click(move |_, _, cx| {
                            m_ok.read(cx).cancel();
                            dismiss(&p_ok, cx);
                        }))
                        .child(small("update-retry", "Retry", Variant::Prominent).on_click(move |_, _, cx| {
                            m_retry.read(cx).retry();
                            dismiss(&p_retry, cx);
                        })),
                )
        }
    }
}

impl Render for UpdatePill {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.model.read(cx).state();
        if state.is_hidden() {
            return div().into_any_element();
        }
        let look = Look::of(cx);
        let fill = tone_color(state.background_tone(), &look);
        let fg = tone_color(state.foreground_tone(), &look);
        let label = state.text();
        let width = text_width(window, &state.max_width_text(), 11.0);
        let model = self.model.clone();
        let is_not_found = state == UpdateState::NotFound;

        let pill = div()
            .id("update-pill")
            .h_flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .py(px(4.))
            .rounded_full()
            .bg(fill)
            .text_color(fg)
            .text_size(px(11.))
            .font_weight(FontWeight::MEDIUM)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                if is_not_found {
                    model.read(cx).cancel();
                } else if this.open {
                    this.open = false;
                    cx.notify();
                } else {
                    this.open_popover(window, cx);
                }
            }))
            .child(div().size(px(14.)).flex().items_center().justify_center().child(Self::badge(&state, &look, fg)))
            .child(div().w(width).overflow_hidden().whitespace_nowrap().text_ellipsis().child(label));

        div()
            .relative()
            .child(pill)
            .when(self.open, |d| {
                let pill_width = f32::from(width) + 36.0;
                let arrow = pill_width / 2.0 - 7.0;
                d.child(div().absolute().top_0().left(px(pill_width)).child(deferred(
                    anchored()
                        .anchor(Anchor::BottomRight)
                        .offset(point(px(0.), px(-6.)))
                        .snap_to_window_with_margin(px(8.))
                        .child(
                            div()
                                .relative()
                                .child(self.popover(&state, &look, cx))
                                .child(div().absolute().right(px(arrow)).bottom(px(-6.)).child(ui::arrow_down(&look))),
                        ),
                )))
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn download_fraction_requires_a_known_length() {
        assert_eq!(download_fraction(None, 10), None);
        assert_eq!(download_fraction(Some(0), 10), None);
        assert_eq!(download_fraction(Some(200), 50), Some(0.25));
        assert_eq!(download_fraction(Some(100), 500), Some(1.0));
    }

    #[test]
    fn every_update_icon_has_a_bundled_glyph() {
        for icon in [
            UpdateIcon::Question,
            UpdateIcon::Refresh,
            UpdateIcon::Package,
            UpdateIcon::PackageFilled,
            UpdateIcon::ArrowDown,
            UpdateIcon::Power,
            UpdateIcon::Info,
            UpdateIcon::Warning,
        ] {
            assert!(super::super::assets::AppAssets.load(icon_of(icon).path().as_ref()).unwrap().is_some());
        }
    }
}

#[cfg(test)]
mod automatic_tests {
    use std::prelude::v1::test;
    use std::time::{Duration, UNIX_EPOCH};

    use angelly_core::config::AutoUpdate;

    use super::automatic_check_wanted;

    #[test]
    fn a_check_is_due_after_a_day() {
        let now = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let ago = |secs: u64| Some(2_000_000_000.0 - secs as f64);
        assert!(automatic_check_wanted(Some(AutoUpdate::Check), None, None, now));
        assert!(automatic_check_wanted(Some(AutoUpdate::Check), None, ago(86_400), now));
        assert!(!automatic_check_wanted(Some(AutoUpdate::Check), None, ago(86_000), now));
    }

    #[test]
    fn off_and_a_declined_prompt_stop_checks() {
        let now = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        assert!(!automatic_check_wanted(Some(AutoUpdate::Off), None, None, now));
        assert!(!automatic_check_wanted(Some(AutoUpdate::Check), Some(false), None, now));
        assert!(automatic_check_wanted(Some(AutoUpdate::Check), Some(true), None, now));
    }
}
