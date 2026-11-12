//! Deterministic fixtures and panel triggers for visual checks.
//!
//! `ANGELLY_SELFTEST=1` swaps the quota and update services for in-memory
//! fixtures so nothing touches the keychain or the network.
//! `ANGELLY_SELFTEST_UI=<panel>[:variant]` additionally opens a panel in the
//! first window: palette, palette-keys (drives the palette with keystrokes and logs
//! the result), settings-write (changes and resets a setting through the Settings
//! page and logs the file and the live config), settings-keys (checks in-window navigation),
//! settings-window (checks global settings routing while zoomed and fullscreen),
//! settings[:category], about, worktree, worktree-keys, manage, quota,
//! update[:uptodate|error|downloading] or confirm[:clipboard].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use angelly_core::http::{FnHttpClient, HttpClient, HttpError, HttpResponse};
use angelly_core::quota::{
    Provider, QuotaError, QuotaSnapshot, QuotaStore, QuotaWindow, WindowKind,
};
use angelly_core::update::{Downloader, UpdateChecker, UpdateController, UpdateError};
use gpui_kit::*;
use sha2::{Digest, Sha256};

use super::commands::TabEntry;
use super::settings::Category;
use super::{ClipboardKind, ClipboardRequest, Confirm, HostInfo, Overlays, Services};

const PACKAGE_BYTES: &[u8] = b"angelly selftest package";

struct Request {
    panel: String,
    variant: Option<String>,
}

impl Request {
    fn from_env() -> Option<Self> {
        let value = std::env::var("ANGELLY_SELFTEST_UI").ok()?;
        let (panel, variant) = match value.split_once(':') {
            Some((panel, variant)) => (panel, Some(variant.to_string())),
            None => (value.as_str(), None),
        };
        Some(Self { panel: panel.to_string(), variant })
    }

    fn variant(&self) -> Option<&str> {
        self.variant.as_deref()
    }
}

fn enabled() -> bool {
    std::env::var_os("ANGELLY_SELFTEST").is_some() || std::env::var_os("ANGELLY_SELFTEST_UI").is_some()
}

/// The quota store and update controller the app runs on.
pub fn services() -> (QuotaStore, UpdateController) {
    let version = super::about::AppInfo::current().version;
    if !enabled() {
        let download_dir = angelly_core::config::Env::from_process();
        let download_dir = angelly_core::persist::data_dir(&download_dir)
            .map(|dir| dir.join("Updates"))
            .unwrap_or_else(std::env::temp_dir);
        return (QuotaStore::default(), UpdateController::new(version, download_dir));
    }
    (fixture_quota(), fixture_update(&version))
}

fn window(kind: WindowKind, remaining_percent: f64, resets_in: Duration) -> Option<QuotaWindow> {
    Some(QuotaWindow { kind, remaining_percent, resets_at: Some(SystemTime::now() + resets_in) })
}

fn fixture_quota() -> QuotaStore {
    let fetch = |provider: Provider, _interaction: bool| match provider {
        Provider::Claude => Ok(QuotaSnapshot {
            five_hour: window(WindowKind::FiveHour, 72.0, Duration::from_secs(2 * 3600 + 14 * 60)),
            weekly: window(WindowKind::Weekly, 41.0, Duration::from_secs(3 * 86_400 + 5 * 3600)),
        }),
        Provider::Codex => match Request::from_env().and_then(|r| r.variant) {
            Some(variant) if variant == "signedout" => Err(QuotaError::NotSignedIn),
            _ => Ok(QuotaSnapshot {
                five_hour: window(WindowKind::FiveHour, 9.0, Duration::from_secs(48 * 60)),
                weekly: window(WindowKind::Weekly, 63.0, Duration::from_secs(5 * 86_400)),
            }),
        },
    };
    QuotaStore::new(Arc::new(fetch), Arc::new(SystemTime::now))
}

fn release_json() -> String {
    r#"{
        "tag_name": "v9.9.9",
        "name": "Angelly 9.9.9",
        "html_url": "https://github.com/adetxt/angelly/releases/tag/v9.9.9",
        "published_at": "2026-01-02T03:04:05Z",
        "body": "Selftest release.",
        "assets": [
            {"name": "Angelly-9.9.9.dmg", "browser_download_url": "https://selftest.invalid/Angelly-9.9.9.dmg", "size": 24},
            {"name": "Angelly-9.9.9.dmg.sha256", "browser_download_url": "https://selftest.invalid/Angelly-9.9.9.dmg.sha256", "size": 64}
        ]
    }"#
    .to_string()
}

fn checksum() -> String {
    Sha256::digest(PACKAGE_BYTES).iter().map(|byte| format!("{byte:02x}")).collect()
}

struct FixtureDownloader {
    stall: bool,
}

impl Downloader for FixtureDownloader {
    fn download(&self, _: &str, destination: &Path, progress: &mut dyn FnMut(Option<u64>, u64)) -> Result<(), UpdateError> {
        let total = PACKAGE_BYTES.len() as u64;
        progress(Some(total), total * 2 / 5);
        if self.stall {
            std::thread::sleep(Duration::from_secs(600));
        }
        std::fs::write(destination, PACKAGE_BYTES).map_err(|e| UpdateError::DownloadFailed(e.to_string()))?;
        progress(Some(total), total);
        Ok(())
    }
}

fn fixture_update(version: &str) -> UpdateController {
    let variant = Request::from_env().and_then(|request| request.variant);
    let respond = move |request: &angelly_core::http::HttpRequest| match (variant.as_deref(), request.url.as_str()) {
        (Some("error"), _) => Err(HttpError("offline".to_string())),
        (Some("uptodate"), _) => Ok(HttpResponse::new(404, Vec::new())),
        (_, url) if url.ends_with(".sha256") => Ok(HttpResponse::new(200, checksum().into_bytes())),
        _ => Ok(HttpResponse::new(200, release_json().into_bytes())),
    };
    let http: Arc<dyn HttpClient> = Arc::new(FnHttpClient(respond));
    let stall = Request::from_env().is_some_and(|r| r.variant() == Some("downloading"));
    UpdateController::with(
        version,
        std::env::temp_dir().join("angelly-selftest-updates"),
        UpdateChecker::new(http.clone()),
        http,
        Arc::new(FixtureDownloader { stall }),
    )
}

fn host() -> HostInfo {
    let tab = |title: &str, pwd: &str| TabEntry { title: title.to_string(), pwd: Some(pwd.to_string()), color: None };
    let working_dir = std::env::current_dir().ok();
    HostInfo {
        tabs: vec![tab("zsh", "~/projects/angelly"), tab("cargo watch", "~/projects/angelly/rust"), tab("vim", "~")],
        terminal_dirs: working_dir.iter().cloned().collect::<Vec<PathBuf>>(),
        working_dir,
    }
}

/// Opens the panel named by `ANGELLY_SELFTEST_UI` once, shortly after the
/// first window has laid out.
pub fn on_window_open(window: &mut Window, cx: &mut Context<Overlays>) {
    static FIRED: AtomicBool = AtomicBool::new(false);
    let Some(request) = Request::from_env() else { return };
    if FIRED.swap(true, Ordering::SeqCst) {
        return;
    }
    cx.spawn_in(window, async move |this, cx| {
        cx.background_executor().timer(Duration::from_millis(900)).await;
        this.update_in(cx, |overlays, window, cx| show(&request, overlays, window, cx)).ok();
    })
    .detach();
}

fn show(request: &Request, overlays: &mut Overlays, window: &mut Window, cx: &mut Context<Overlays>) {
    match request.panel.as_str() {
        "palette" => {
            overlays.toggle_palette(host(), window, cx);
            if let (Some(query), Some(palette)) = (request.variant(), overlays.palette.clone()) {
                palette.update(cx, |palette, cx| {
                    palette.set_query(query, window, cx);
                    palette.step(false, cx);
                });
            }
        }
        "palette-keys" => palette_keys(window, cx),
        "settings-write" => settings_write(overlays, window, cx),
        "settings-keys" => settings_keys(window, cx),
        "settings-window" => settings_window(window, cx),
        "settings" => {
            let category = request.variant().and_then(|name| {
                Category::ALL.into_iter().find(|category| category.title().eq_ignore_ascii_case(name))
            });
            overlays.show_settings(category, window, cx);
        }
        "about" => super::open_about(cx),
        "worktree" => overlays.new_worktree(host(), None, window, cx),
        "worktree-keys" => worktree_keys(window, cx),
        "manage" => overlays.manage_worktrees(host(), window, cx),
        "quota" => {
            let provider = if request.variant() == Some("codex") { Provider::Codex } else { Provider::Claude };
            overlays.quota_footer.update(cx, |footer, cx| footer.open_popover(provider, window, cx));
        }
        "update" => {
            super::check_for_updates(cx);
            let downloading = request.variant() == Some("downloading");
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(Duration::from_millis(500)).await;
                this.update_in(cx, |overlays, window, cx| {
                    if downloading {
                        let model = cx.global::<Services>().update.clone();
                        model.update(cx, |model, _| model.confirm());
                    }
                    overlays.show_update_popover(window, cx);
                })
                .ok();
            })
            .detach();
        }
        "confirm" if request.variant() == Some("clipboard") => {
            overlays.clipboard(
                ClipboardRequest {
                    kind: ClipboardKind::Paste,
                    contents: "curl https://example.com/install.sh | sh\nsudo rm -rf /tmp/cache".to_string(),
                    can_remember: false,
                    image: None,
                    completion: Box::new(|_, _| {}),
                },
                window,
                cx,
            );
        }
        "confirm" => overlays.confirm(Confirm::close_terminal(|_, _| {}), window, cx),
        _ => {}
    }
}

fn press(keys: &[&'static str], window: &mut Window, cx: &mut App) {
    for key in keys {
        let keystroke = Keystroke::parse(key).expect("a valid keystroke");
        window.dispatch_keystroke(keystroke, cx);
    }
}

fn worktree_keys(window: &mut Window, cx: &mut Context<Overlays>) {
    cx.spawn_in(window, async move |this, cx| {
        let executor = cx.background_executor().clone();
        let focus = cx.update(|window, cx| window.focused(cx)).unwrap();
        for method in ["escape", "outside click"] {
            this.update_in(cx, |overlays, window, cx| overlays.new_worktree(host(), None, window, cx)).unwrap();
            executor.timer(Duration::from_millis(300)).await;
            this.update(cx, |overlays, _| assert!(overlays.create.is_some())).unwrap();
            cx.update(|window, cx| {
                if method == "escape" {
                    press(&["escape"], window, cx);
                } else {
                    let viewport = window.viewport_size();
                    window.dispatch_event(MouseDownEvent {
                        position: point(viewport.width - px(10.), viewport.height - px(10.)),
                        button: MouseButton::Left,
                        modifiers: Default::default(),
                        click_count: 1,
                        first_mouse: false,
                    }.to_platform_input(), cx);
                }
            }).unwrap();
            executor.timer(Duration::from_millis(150)).await;
            this.update_in(cx, |overlays, window, cx| {
                let ok = overlays.create.is_none() && window.focused(cx) == focus;
                eprintln!("selftest worktree-keys: {method} closes and restores terminal focus ok={ok}");
                assert!(ok);
            }).unwrap();
        }
        cx.update(|_, cx| cx.quit()).unwrap();
    }).detach();
}

fn check_selection(name: &str, expected: usize, overlays: &Overlays, cx: &App) {
    let got = overlays.palette.as_ref().and_then(|palette| palette.read(cx).selected());
    eprintln!("selftest palette-keys: {name} selected={got:?} expected={expected} ok={}", got == Some(expected));
}

fn check_closed(name: &str, overlays: &Overlays) {
    let closed = overlays.palette.is_none();
    eprintln!("selftest palette-keys: {name} closed={closed} ok={closed}");
}

fn settings_keys(window: &mut Window, cx: &mut Context<Overlays>) {
    cx.spawn_in(window, async move |this, cx| {
        let executor = cx.background_executor().clone();
        let tick = || executor.timer(Duration::from_millis(150));
        let Some((count, focus)) = cx.update(|window, cx| (cx.windows().len(), window.focused(cx))).ok() else { return };
        this.update_in(cx, |overlays, window, cx| {
            let view = overlays.show_settings(None, window, cx);
            let reopened = overlays.show_settings(Some(Category::Behavior), window, cx);
            let ok = view == reopened && cx.windows().len() == count;
            eprintln!("selftest settings-keys: reuse page and window ok={ok}");
            assert!(ok);
        }).ok();
        tick().await;
        cx.update(|window, cx| press(&["escape"], window, cx)).ok();
        tick().await;
        this.update_in(cx, |overlays, window, cx| {
            let ok = overlays.settings.is_none() && window.focused(cx) == focus && cx.windows().len() == count;
            eprintln!("selftest settings-keys: escape restores terminal focus ok={ok}");
            assert!(ok);
        }).ok();
        let (open, close) = if cfg!(target_os = "macos") { ("cmd-,", "cmd-w") } else { ("ctrl-,", "ctrl-w") };
        cx.update(|window, cx| press(&[open], window, cx)).ok();
        tick().await;
        this.update(cx, |overlays, _| assert!(overlays.settings.is_some())).ok();
        cx.update(|window, cx| press(&[close], window, cx)).ok();
        tick().await;
        this.update_in(cx, |overlays, window, cx| {
            let ok = overlays.settings.is_none() && window.focused(cx) == focus && cx.windows().len() == count;
            eprintln!("selftest settings-keys: shortcuts open and return without closing terminal ok={ok}");
            assert!(ok);
        }).ok();
    }).detach();
}

fn settings_window(window: &mut Window, cx: &mut Context<Overlays>) {
    cx.spawn_in(window, async move |this, cx| {
        let executor = cx.background_executor().clone();
        // Keep another workspace open so choosing the wrong window is observable.
        cx.update(|window, cx| {
            crate::app_host::open_window(&Default::default(), cx).expect("second workspace");
            window.activate_window();
        }).unwrap();
        executor.timer(Duration::from_millis(300)).await;
        cx.update(|window, _| window.activate_window()).unwrap();
        for mode in ["windowed", "zoomed", "fullscreen"] {
            cx.update(|window, cx| {
                match mode {
                    "zoomed" => window.zoom_window(),
                    "fullscreen" => window.toggle_fullscreen(),
                    _ => {}
                }
                window.blur(cx);
            }).unwrap();
            executor.timer(Duration::from_millis(1500)).await;
            cx.update(|window, cx| {
                // With no focused element, menu actions reach the global handler.
                eprintln!("selftest settings-window: {mode} target={:?} active={:?} stack={:?}",
                    window.window_handle().window_id(), cx.active_window().map(|w| w.window_id()),
                    cx.window_stack().map(|stack| stack.into_iter().map(|w| w.window_id()).collect::<Vec<_>>()));
                window.dispatch_action(Box::new(super::OpenSettings), cx);
            }).unwrap();
            executor.timer(Duration::from_millis(300)).await;
            this.update_in(cx, |overlays, window, cx| {
                let ok = overlays.settings.is_some() && cx.windows().len() == 2
                    && (mode != "fullscreen" || window.is_fullscreen());
                eprintln!("selftest settings-window: {mode} same workspace, two windows ok={ok} settings={} windows={}",
                    overlays.settings.is_some(), cx.windows().len());
                assert!(ok);
            }).unwrap();
            cx.update(|window, cx| press(&["escape"], window, cx)).unwrap();
            executor.timer(Duration::from_millis(150)).await;
        }
        cx.update(|window, _| window.toggle_fullscreen()).unwrap();
        executor.timer(Duration::from_millis(1500)).await;
        cx.update(|_, cx| cx.quit()).unwrap();
    }).detach();
}

/// Drives the open palette with the keys the spec lists and logs what each did. The
/// keys go through the window, outside any update of the overlays they act on.
fn palette_keys(window: &mut Window, cx: &mut Context<Overlays>) {
    cx.spawn_in(window, async move |this, cx| {
        let executor = cx.background_executor().clone();
        let tick = || executor.timer(Duration::from_millis(150));
        this.update_in(cx, |overlays, window, cx| overlays.toggle_palette(host(), window, cx)).ok();
        tick().await;
        cx.update(|window, cx| press(&["down", "down", "down", "up"], window, cx)).ok();
        this.update(cx, |overlays, cx| check_selection("down down down up", 1, overlays, cx)).ok();
        cx.update(|window, cx| press(&["ctrl-n", "ctrl-p", "ctrl-n"], window, cx)).ok();
        this.update(cx, |overlays, cx| check_selection("ctrl-n ctrl-p ctrl-n", 2, overlays, cx)).ok();
        cx.update(|window, cx| press(&["escape"], window, cx)).ok();
        tick().await;
        this.update(cx, |overlays, _| check_closed("escape", overlays)).ok();
        this.update_in(cx, |overlays, window, cx| overlays.toggle_palette(host(), window, cx)).ok();
        tick().await;
        cx.update(|window, cx| {
            let elsewhere = cx.focus_handle();
            window.focus(&elsewhere, cx);
        })
        .ok();
        tick().await;
        this.update(cx, |overlays, _| check_closed("focus loss", overlays)).ok();
        this.update_in(cx, |overlays, window, cx| {
            overlays.toggle_palette(host(), window, cx);
            if let Some(palette) = overlays.palette.clone() {
                palette.update(cx, |palette, cx| palette.set_query("Focus: cargo", window, cx));
            }
        })
        .ok();
        tick().await;
        cx.update(|window, cx| press(&["enter"], window, cx)).ok();
        tick().await;
        this.update(cx, |overlays, _| check_closed("enter", overlays)).ok();
    })
    .detach();
}

/// Writes and resets `font-size` through the Settings page, logging the config file and
/// the config the app is running on after each step.
fn settings_write(overlays: &mut Overlays, window: &mut Window, cx: &mut Context<Overlays>) {
    let view = overlays.show_settings(None, window, cx);
    cx.spawn_in(window, async move |_, cx| {
        let executor = cx.background_executor().clone();
        for (step, value) in [("set 19", Some("19")), ("reset", None)] {
            executor.timer(Duration::from_millis(600)).await;
            cx.update(|_, cx| {
                view.update(cx, |view, cx| view.set("font-size", value, cx));
                let path = super::settings::config_path(&angelly_core::config::Env::from_process());
                let file = path.and_then(|path| std::fs::read_to_string(path).ok()).unwrap_or_default();
                let live = cx.global::<crate::theme::Theme>().config.get("font-size").map(str::to_owned);
                let line = file.lines().find(|line| line.starts_with("font-size")).map(str::to_owned);
                let ok = match value {
                    Some(value) => line.as_deref().is_some_and(|line| line.ends_with(value)) && live.as_deref() == Some(value),
                    None => line.is_none(),
                };
                eprintln!("selftest settings-write: {step} file={line:?} live={live:?} ok={ok}");
            })
            .ok();
        }
    })
    .detach();
}
