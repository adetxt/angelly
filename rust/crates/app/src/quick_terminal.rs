//! The quick terminal: one terminal in a borderless panel that slides in from a screen
//! edge on a hot key and out again when it loses focus (spec section 15).
//!
//! The panel is a GPUI window so the terminal view works unchanged; the platform layer
//! positions, layers and orders it. The session survives hiding: only the panel's
//! visibility changes.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use gpui_kit::*;

use crate::terminal_view::TerminalView;

#[cfg(target_os = "macos")]
pub use imp::{selftest, show, toggle, view};

#[cfg(not(target_os = "macos"))]
pub fn toggle(_: &mut App) {}

#[cfg(not(target_os = "macos"))]
pub fn show(_: &mut App) -> Result<(), String> {
    Err("The quick terminal is not available on this platform.".into())
}

#[cfg(not(target_os = "macos"))]
pub fn view(_: &App) -> Option<Entity<TerminalView>> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn selftest(_: &mut App) {}

#[cfg(target_os = "macos")]
mod imp {
    use std::time::Duration;

    use super::*;
    use crate::platform::menus;
    use crate::platform::quick_terminal::geometry::{ease_in, Position, Rect, Size};
    use crate::platform::quick_terminal::panel::{Panel, PreviousApp, ScreenChoice, Space};
    use crate::theme::Theme;

    const FRAME: Duration = Duration::from_millis(16);

    struct Settings {
        position: Position,
        size: Size,
        screen: ScreenChoice,
        space: Space,
        autohide: bool,
        duration: Duration,
    }

    impl Settings {
        fn load(cx: &App) -> Self {
            let config = &cx.global::<Theme>().config;
            let get = |key| config.get(key);
            Self {
                position: get("quick-terminal-position").and_then(Position::parse).unwrap_or_default(),
                size: get("quick-terminal-size").map(Size::parse).unwrap_or_default(),
                screen: get("quick-terminal-screen").and_then(ScreenChoice::parse).unwrap_or_default(),
                space: get("quick-terminal-space-behavior").and_then(Space::parse).unwrap_or_default(),
                autohide: get("quick-terminal-autohide") != Some("false"),
                duration: Duration::from_secs_f64(
                    get("quick-terminal-animation-duration")
                        .and_then(|value| value.parse::<f64>().ok())
                        .unwrap_or(0.2)
                        .clamp(0.0, 5.0),
                ),
            }
        }
    }

    struct QuickTerminal {
        window: AnyWindowHandle,
        panel: Panel,
        view: Entity<TerminalView>,
        visible: bool,
        /// Set once the panel has been key, so the activation change that comes with showing
        /// it is not mistaken for losing focus.
        was_key: bool,
        previous: Option<PreviousApp>,
        /// Incremented per animation; a running animation stops when it no longer matches.
        generation: u64,
    }

    impl Global for QuickTerminal {}

    struct Root {
        terminal: Entity<TerminalView>,
    }

    impl Render for Root {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(cx.global::<Theme>().background()).child(self.terminal.clone())
        }
    }

    pub fn view(cx: &App) -> Option<Entity<TerminalView>> {
        cx.try_global::<QuickTerminal>().map(|quick| quick.view.clone())
    }

    pub fn toggle(cx: &mut App) {
        if cx.try_global::<QuickTerminal>().is_some_and(|quick| quick.visible) {
            hide(cx);
        } else {
            let _ = show(cx);
        }
    }

    pub fn show(cx: &mut App) -> Result<(), String> {
        let settings = Settings::load(cx);
        let frames = prepare(&settings, cx)?;
        let quick = cx.global_mut::<QuickTerminal>();
        if quick.visible {
            return Ok(());
        }
        quick.visible = true;
        quick.was_key = false;
        quick.previous = PreviousApp::capture();
        quick.panel.set_frame(frames.hidden, 0.0);
        quick.panel.show();
        let (window, view) = (quick.window, quick.view.clone());
        cx.activate(true);
        let _ = window.update(cx, |_, window, cx| {
            let focus = view.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        });
        animate(frames.hidden, frames.shown, 0.0, 1.0, settings.duration, true, cx);
        menus::install(cx);
        Ok(())
    }

    fn quick_mut(cx: &mut App) -> Option<&mut QuickTerminal> {
        cx.has_global::<QuickTerminal>().then(|| cx.global_mut::<QuickTerminal>())
    }

    fn hide(cx: &mut App) {
        let settings = Settings::load(cx);
        let Some(quick) = quick_mut(cx) else { return };
        if !quick.visible {
            return;
        }
        quick.visible = false;
        let current = quick.panel.frame();
        let hidden = geometry_frames(&settings).map_or(current, |frames| frames.hidden);
        if let Some(previous) = quick.previous.take() {
            previous.restore();
        }
        animate(current, hidden, 1.0, 0.0, settings.duration, false, cx);
    }

    struct Frames {
        hidden: Rect,
        shown: Rect,
    }

    fn geometry_frames(settings: &Settings) -> Option<Frames> {
        let area = crate::platform::quick_terminal::panel::screen_area(settings.screen)?;
        let size = settings.size.calculate(settings.position, area);
        let rect = |(x, y): (f64, f64)| Rect::new(x, y, size.0, size.1);
        Some(Frames {
            hidden: rect(settings.position.hidden_origin(size, area)),
            shown: rect(settings.position.final_origin(size, area)),
        })
    }

    /// Makes sure the panel exists and is configured, and returns where it slides between.
    fn prepare(settings: &Settings, cx: &mut App) -> Result<Frames, String> {
        let frames = geometry_frames(settings).ok_or("No screen is available.")?;
        if cx.try_global::<QuickTerminal>().is_none() {
            create(&frames.shown, cx)?;
        }
        cx.global::<QuickTerminal>().panel.configure(settings.space);
        Ok(frames)
    }

    fn create(frame: &Rect, cx: &mut App) -> Result<(), String> {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(frame.width as f32), px(frame.height as f32)));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: None,
            kind: WindowKind::PopUp,
            show: false,
            focus: false,
            is_movable: false,
            ..Default::default()
        };
        let mut terminal = None;
        let (window, _) = gpui_kit::open_window(options, cx, |window, cx| {
            let created = cx.new(|cx| TerminalView::new(None, cx));
            terminal = Some(created.clone());
            let root = cx.new(|_| Root { terminal: created });
            observe_activation(&root, window, cx);
            root
        })
        .map_err(|error| error.to_string())?;
        let view = terminal.ok_or("The quick terminal could not start a terminal.")?;
        let panel = window
            .update(cx, |_, window, _| Panel::of(window))
            .ok()
            .flatten()
            .ok_or("The quick terminal has no native window.")?;
        cx.set_global(QuickTerminal { window, panel, view, visible: false, was_key: false, previous: None, generation: 0 });
        Ok(())
    }

    /// Hides the panel when it stops being the key window, if `quick-terminal-autohide` allows.
    fn observe_activation(root: &Entity<Root>, window: &mut Window, cx: &mut App) {
        let autohide = Settings::load(cx).autohide;
        root.update(cx, |_, cx| {
            cx.observe_window_activation(window, move |_, window, cx| {
                let Some(quick) = quick_mut(cx) else { return };
                if window.is_window_active() {
                    quick.was_key = true;
                } else if autohide && quick.was_key && quick.visible {
                    cx.defer(hide);
                }
            })
            .detach();
        });
    }

    /// Moves the panel from `from` to `to` while fading `alpha_from` to `alpha_to`. The panel
    /// is ordered out at the end of a hiding animation and settles at its resting level at
    /// the end of a showing one.
    fn animate(from: Rect, to: Rect, alpha_from: f64, alpha_to: f64, duration: Duration, showing: bool, cx: &mut App) {
        let quick = cx.global_mut::<QuickTerminal>();
        quick.generation += 1;
        let generation = quick.generation;
        let steps = (duration.as_secs_f64() / FRAME.as_secs_f64()).ceil() as u32;
        cx.spawn(async move |cx| {
            for step in 1..=steps {
                cx.background_executor().timer(FRAME).await;
                let t = ease_in(f64::from(step) / f64::from(steps));
                let lerp = |a: f64, b: f64| a + (b - a) * t;
                let frame = Rect::new(
                    lerp(from.x, to.x),
                    lerp(from.y, to.y),
                    lerp(from.width, to.width),
                    lerp(from.height, to.height),
                );
                let alpha = lerp(alpha_from, alpha_to);
                let live = cx.update(|cx| match cx.try_global::<QuickTerminal>() {
                    Some(quick) if quick.generation == generation => {
                        quick.panel.set_frame(frame, alpha);
                        true
                    }
                    _ => false,
                });
                if !live {
                    return;
                }
            }
            cx.update(|cx| {
                let Some(quick) = cx.try_global::<QuickTerminal>().filter(|quick| quick.generation == generation) else {
                    return;
                };
                quick.panel.set_frame(to, alpha_to);
                if showing {
                    quick.panel.settle();
                } else {
                    quick.panel.hide();
                    menus::install(cx);
                }
            });
        })
        .detach();
    }

    /// Test hook behind `ANGELLY_SELFTEST_QUICK`: builds the panel without ordering it onto the
    /// screen, then prints its layout and native state for every position.
    pub fn selftest(cx: &mut App) {
        let mut settings = Settings::load(cx);
        let mut report = Vec::new();
        for position in [Position::Top, Position::Bottom, Position::Left, Position::Right, Position::Center] {
            settings.position = position;
            let Some(frames) = geometry_frames(&settings) else {
                println!("quick-terminal selftest: no screen");
                return;
            };
            report.push(format!("{position:?}: hidden {:?} shown {:?}", frames.hidden, frames.shown));
        }
        let Ok(_) = prepare(&settings, cx) else {
            println!("quick-terminal selftest: panel creation failed");
            return;
        };
        let quick = cx.global::<QuickTerminal>();
        println!("quick-terminal selftest: visible={} view={}", quick.panel.is_visible(), quick.visible);
        for line in report {
            println!("quick-terminal selftest: {line}");
        }
    }
}
