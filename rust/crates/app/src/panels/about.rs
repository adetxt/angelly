//! The About window.

use std::time::Duration;

use gpui_kit::component::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::assets::{app_icon, ICON_VARIANTS};
use super::ui::{self, text, Look, Variant};

const GITHUB_URL: &str = "https://github.com/adetxt/angelly";
const GHOSTTY_URL: &str = "https://github.com/ghostty-org/ghostty";
const DOCS_URL: &str = "https://ghostty.org/docs";

pub const WINDOW_SIZE: (f32, f32) = (320.0, 392.0);

/// Version facts shown in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInfo {
    pub version: String,
    pub build: Option<String>,
    pub commit: Option<String>,
    pub copyright: Option<String>,
}

impl AppInfo {
    /// The marketing version of the Swift app this port tracks, used when the app does not
    /// run from a bundle.
    pub const VERSION: &'static str = "0.15.0";

    /// The bundle's `CFBundleShortVersionString` and `CFBundleVersion` when the app runs
    /// from a bundle, else the build-time version.
    pub fn current() -> Self {
        Self::resolve(bundle_string("CFBundleShortVersionString"), bundle_string("CFBundleVersion"))
    }

    fn resolve(short_version: Option<String>, build: Option<String>) -> Self {
        let version = short_version
            .filter(|version| !version.is_empty() && !version.starts_with('@'))
            .unwrap_or_else(|| option_env!("ANGELLY_VERSION").unwrap_or(Self::VERSION).to_string());
        let build = build.filter(|build| !build.is_empty() && !build.starts_with('@')).unwrap_or_else(|| version.clone());
        Self { build: Some(build), version, commit: option_env!("ANGELLY_COMMIT").map(str::to_string), copyright: None }
    }
}

#[cfg(target_os = "macos")]
fn bundle_string(key: &str) -> Option<String> {
    use objc2_foundation::{NSBundle, NSString};
    let value = NSBundle::mainBundle().objectForInfoDictionaryKey(&NSString::from_str(key))?;
    Some(value.downcast::<NSString>().ok()?.to_string())
}

#[cfg(not(target_os = "macos"))]
fn bundle_string(_: &str) -> Option<String> {
    None
}

/// How the version row reads and where it links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionRow {
    Stable { version: String },
    Tip,
    Other(String),
}

impl VersionRow {
    pub fn parse(version: &str) -> Self {
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| digits(p)) {
            return Self::Stable { version: version.to_string() };
        }
        let is_hash = (7..=40).contains(&version.len()) && version.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if is_hash { Self::Tip } else { Self::Other(version.to_string()) }
    }

    pub fn text(&self) -> String {
        match self {
            Self::Stable { version } => version.clone(),
            Self::Tip => "Tip Release".to_string(),
            Self::Other(text) => text.clone(),
        }
    }

    pub fn url(&self) -> Option<String> {
        match self {
            Self::Stable { version } => Some(format!("{GITHUB_URL}/releases/tag/v{version}")),
            _ => None,
        }
    }
}

pub fn commit_url(commit: &str) -> String {
    format!("{GITHUB_URL}/commits/{commit}")
}

/// The variant after `current`, wrapping; the first advance from the initial
/// state lands on the second variant.
pub fn next_variant(current: Option<usize>, count: usize) -> usize {
    (current.unwrap_or(0) + 1) % count
}

pub struct AboutView {
    info: AppInfo,
    current: Option<usize>,
    previous: Option<usize>,
    generation: usize,
    hovering: bool,
    menu: Option<Point<Pixels>>,
    focus: FocusHandle,
}

impl AboutView {
    pub fn new(info: AppInfo, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            let alive = this.update(cx, |this, cx| {
                if !this.hovering {
                    this.advance(cx);
                }
            });
            if alive.is_err() {
                break;
            }
        })
        .detach();
        Self { info, current: None, previous: None, generation: 0, hovering: false, menu: None, focus }
    }

    fn advance(&mut self, cx: &mut Context<Self>) {
        self.previous = Some(self.current.unwrap_or(0));
        self.current = Some(next_variant(self.current, ICON_VARIANTS.len()));
        self.generation += 1;
        cx.notify();
    }

    fn icon_layer(index: usize, opacity: f32) -> Div {
        div()
            .absolute()
            .inset_0()
            .opacity(opacity)
            .flex()
            .items_center()
            .justify_center()
            .children(app_icon(ICON_VARIANTS[index]).map(|image| img(image).h(px(128.)).w(px(128.))))
    }

    fn icon_stack(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.current.unwrap_or(0);
        let mut stack = div()
            .id("about-icon")
            .relative()
            .w_full()
            .h(px(128.))
            .flex_none()
            .on_hover(cx.listener(|this, hovering: &bool, _, cx| {
                this.hovering = *hovering;
                cx.notify();
            }))
            .on_click(cx.listener(|this, _, _, cx| this.advance(cx)))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if this.current.is_some() {
                        this.menu = Some(event.position);
                        cx.notify();
                    }
                }),
            );
        stack = match self.previous {
            Some(previous) => stack
                .child(Self::icon_layer(previous, 1.0).with_animation(
                    ("about-out", self.generation),
                    Animation::new(Duration::from_millis(500)).with_easing(ease_in_out),
                    |layer, delta| layer.opacity(1.0 - delta),
                ))
                .child(Self::icon_layer(shown, 0.0).with_animation(
                    ("about-in", self.generation),
                    Animation::new(Duration::from_millis(500)).with_easing(ease_in_out),
                    |layer, delta| layer.opacity(delta),
                )),
            None => stack.child(Self::icon_layer(shown, 1.0)),
        };
        stack
    }
}

fn property_row(label: &str, value: String, url: Option<String>, look: &Look, id: &'static str) -> Div {
    let value_view = div()
        .id(id)
        .w(px(125.))
        .pl(px(2.))
        .opacity(0.8)
        .font_family("Menlo")
        .text_size(px(11.))
        .when_some(url, |d, url| d.cursor_pointer().on_click(move |_, _, cx| cx.open_url(&url)))
        .child(value);
    div()
        .h_flex()
        .gap(px(4.))
        .justify_center()
        .items_center()
        .text_size(px(text::CALLOUT))
        .child(div().w(px(126.)).pr(px(2.)).text_right().text_color(look.primary).child(label.to_string()))
        .child(value_view)
}

impl Render for AboutView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let look = Look::of(cx);
        let version = VersionRow::parse(&self.info.version);
        let mut rows = div().v_flex().gap(px(2.)).w_full();
        rows = rows.child(property_row("Version", version.text(), version.url(), &look, "about-version"));
        if let Some(build) = &self.info.build {
            rows = rows.child(property_row("Build", build.clone(), None, &look, "about-build"));
        }
        if let Some(commit) = self.info.commit.as_ref().filter(|c| !c.is_empty()) {
            rows = rows.child(property_row("Commit", commit.clone(), Some(commit_url(commit)), &look, "about-commit"));
        }

        let menu = self.menu.map(|position| {
            let variant = ICON_VARIANTS[self.current.unwrap_or(0)];
            deferred(
                anchored().position(position).snap_to_window_with_margin(px(8.)).child(
                    ui::menu(&look).child(
                        ui::menu_item("about-copy-icon", "Copy Icon Config", false, &look).on_click(cx.listener(
                            move |this, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(format!("macos-icon = {variant}")));
                                this.menu = None;
                                cx.notify();
                            },
                        )),
                    ).on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    })),
                ),
            )
        });

        let _ = window;
        div()
            .id("about")
            .key_context("About")
            .track_focus(&self.focus)
            .size_full()
            .v_flex()
            .items_center()
            .bg(look.background)
            .text_color(look.primary)
            .font_family(ui::FONT).line_height(relative(ui::LINE_HEIGHT))
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, _| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                }
            }))
            .pt(px(40.))
            .px(px(32.))
            .pb(px(32.))
            .gap(px(8.))
            .child(self.icon_stack(cx))
            .child(
                div()
                    .v_flex()
                    .items_center()
                    .w_full()
                    .gap(px(32.))
                    .child(
                        div()
                            .v_flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).child("Angelly"))
                            .child(
                                ui::link("about-ghostty", "Powered by Ghostty", &look, text::CAPTION)
                                    .on_click(|_, _, cx| cx.open_url(GHOSTTY_URL)),
                            ),
                    )
                    .child(rows)
                    .child(
                        div()
                            .h_flex()
                            .gap(px(8.))
                            .child(
                                ui::button("about-docs", "Docs", &look, Variant::Regular, false, true)
                                    .on_click(|_, _, cx| cx.open_url(DOCS_URL)),
                            )
                            .child(
                                ui::button("about-github", "GitHub", &look, Variant::Regular, false, true)
                                    .on_click(|_, _, cx| cx.open_url(GITHUB_URL)),
                            ),
                    )
                    .children(self.info.copyright.clone().map(|copyright| {
                        div()
                            .w_full()
                            .opacity(0.8)
                            .text_size(px(text::CAPTION))
                            .text_center()
                            .child(copyright)
                    })),
            )
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use super::*;

    #[test]
    fn the_bundle_version_wins_over_the_constant() {
        let info = AppInfo::resolve(Some("1.4.2".into()), Some("57".into()));
        assert_eq!((info.version.as_str(), info.build.as_deref()), ("1.4.2", Some("57")));
    }

    #[test]
    fn a_missing_or_unfilled_bundle_falls_back() {
        let info = AppInfo::resolve(Some("@VERSION@".into()), None);
        assert!(!info.version.starts_with('@'));
        assert_eq!(info.build.as_deref(), Some(info.version.as_str()));
        assert_eq!(AppInfo::resolve(None, None).version, info.version);
    }

    #[test]
    fn stable_versions_link_to_their_release() {
        let row = VersionRow::parse("0.15.0");
        assert_eq!(row.text(), "0.15.0");
        assert_eq!(row.url().as_deref(), Some("https://github.com/adetxt/angelly/releases/tag/v0.15.0"));
    }

    #[test]
    fn commit_hashes_read_as_tip_releases() {
        assert_eq!(VersionRow::parse("a1b2c3d"), VersionRow::Tip);
        assert_eq!(VersionRow::Tip.text(), "Tip Release");
        assert_eq!(VersionRow::Tip.url(), None);
        assert_eq!(VersionRow::parse("a1b2c3"), VersionRow::Other("a1b2c3".into()));
    }

    #[test]
    fn other_versions_are_shown_verbatim() {
        assert_eq!(VersionRow::parse("1.2").text(), "1.2");
        assert_eq!(VersionRow::parse("1.2.x"), VersionRow::Other("1.2.x".into()));
    }

    #[test]
    fn commit_links_point_at_the_repository() {
        assert_eq!(commit_url("abc1234"), "https://github.com/adetxt/angelly/commits/abc1234");
    }

    #[test]
    fn icons_cycle_and_wrap() {
        let count = ICON_VARIANTS.len();
        assert_eq!(next_variant(None, count), 1);
        assert_eq!(next_variant(Some(1), count), 2);
        assert_eq!(next_variant(Some(count - 1), count), 0);
    }

    #[test]
    fn every_icon_variant_has_a_bitmap_and_config_value() {
        use angelly_core::config::{ColorScheme, Config, Env, Platform};
        use crate::platform::app_icon::{variant_for, Variant};

        let env = Env { platform: Platform::MacOs, home: None, xdg_config_home: None, resources_dir: None };
        for variant in ICON_VARIANTS {
            assert!(app_icon(variant).is_some(), "{variant}");
            let config = Config::from_str(&format!("macos-icon = {variant}\n"), &env, ColorScheme::Dark);
            let expected = if *variant == "official" { Variant::Official } else { Variant::Named(variant) };
            assert_eq!(variant_for(&config), expected, "{variant}");
        }
        assert!(app_icon("custom").is_none());
    }
}
