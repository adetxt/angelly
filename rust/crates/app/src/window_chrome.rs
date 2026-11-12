//! How a window looks and starts, from the config: titlebar style, decorations,
//! initial size in cells, maximize and fullscreen, the light or dark appearance, the
//! color space and the cell-stepped resizing.

use angelly_core::config::{Config, MacTitlebarStyle, WindowDecoration, WindowTheme};
use gpui_kit::*;

use crate::theme::Theme;
use crate::workspace::{Workspace, TITLEBAR_HEIGHT};

/// Space the traffic lights take at the leading edge of a transparent titlebar.
const TRAFFIC_LIGHTS: f32 = 78.0;
/// Space left at the leading edge when the system draws its own titlebar.
const NATIVE_INSET: f32 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
}

/// What the config says about the window frame and behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The titlebar style after `window-decoration = none` is applied.
    pub style: MacTitlebarStyle,
    pub maximize: bool,
    pub fullscreen: bool,
    pub non_native_fullscreen: bool,
    pub step_resize: bool,
    pub appearance: Appearance,
    pub color_space: ColorSpace,
    /// Initial size as columns and rows, when both are set.
    pub cells: Option<(u32, u32)>,
}

fn flag(config: &Config, key: &str, default: bool) -> bool {
    match config.get(key) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

impl Spec {
    pub fn from_config(config: &Config, dark_background: bool) -> Self {
        let style = match config.window_decoration() {
            WindowDecoration::None => MacTitlebarStyle::Hidden,
            _ => config.macos_titlebar_style(),
        };
        let appearance = match config.window_theme() {
            WindowTheme::System => Appearance::System,
            WindowTheme::Light => Appearance::Light,
            WindowTheme::Dark => Appearance::Dark,
            WindowTheme::Auto | WindowTheme::Ghostty => {
                if dark_background {
                    Appearance::Dark
                } else {
                    Appearance::Light
                }
            }
        };
        let color_space = match config.get("window-colorspace") {
            Some("display-p3") => ColorSpace::DisplayP3,
            _ => ColorSpace::Srgb,
        };
        let cells = match (config.window_width(), config.window_height()) {
            (0, _) | (_, 0) => None,
            (width, height) => Some((width, height)),
        };
        Self {
            style,
            maximize: flag(config, "maximize", false),
            fullscreen: flag(config, "fullscreen", false),
            non_native_fullscreen: !matches!(config.get("macos-non-native-fullscreen"), None | Some("false")),
            step_resize: flag(config, "window-step-resize", false),
            appearance,
            color_space,
            cells,
        }
    }

    pub fn of(cx: &App) -> Self {
        let theme = cx.global::<Theme>();
        Self::from_config(&theme.config, theme.dark)
    }

    /// The titlebar window option. A hidden titlebar has none; the system draws an opaque
    /// one only for the native style.
    pub fn titlebar(&self, title: &str) -> Option<TitlebarOptions> {
        match self.style {
            MacTitlebarStyle::Hidden => None,
            MacTitlebarStyle::Native => Some(TitlebarOptions {
                title: Some(title.to_string().into()),
                // Windows has no system titlebar to show: the app draws the caption buttons.
                appears_transparent: cfg!(windows),
                traffic_light_position: None,
            }),
            MacTitlebarStyle::Transparent | MacTitlebarStyle::Tabs => Some(TitlebarOptions {
                title: Some(title.to_string().into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.), px(11.))),
            }),
        }
    }

    /// The window bounds option: fullscreen and maximize win over the frame.
    pub fn window_bounds(&self, frame: Bounds<Pixels>) -> WindowBounds {
        if self.fullscreen && !self.non_native_fullscreen {
            WindowBounds::Fullscreen(frame)
        } else if self.maximize {
            WindowBounds::Maximized(frame)
        } else {
            WindowBounds::Windowed(frame)
        }
    }

    /// Height of the row the app draws at the top of the window.
    pub fn titlebar_height(&self) -> f32 {
        if self.style == MacTitlebarStyle::Hidden {
            0.0
        } else {
            TITLEBAR_HEIGHT
        }
    }

    /// Where the app's own titlebar content starts, clear of the traffic lights.
    pub fn leading_inset(&self) -> f32 {
        if cfg!(not(target_os = "macos")) {
            return NATIVE_INSET;
        }
        match self.style {
            MacTitlebarStyle::Native => NATIVE_INSET,
            _ => TRAFFIC_LIGHTS,
        }
    }

    /// Whether the tab bar lives in the titlebar row.
    pub fn tabs_in_titlebar(&self) -> bool {
        self.style == MacTitlebarStyle::Tabs
    }
}

/// The window size that shows `cells` columns and rows of the given cell size, plus the
/// chrome around the terminal.
pub fn content_size(cells: (u32, u32), cell: (f32, f32), padding: (f32, f32), chrome: (f32, f32)) -> (f32, f32) {
    (
        cells.0 as f32 * cell.0 + padding.0 + chrome.0,
        cells.1 as f32 * cell.1 + padding.1 + chrome.1,
    )
}

impl Workspace {
    /// Applies the window settings that need the cell size: the initial size in cells
    /// (once, for a window without a saved frame) and the resize steps.
    pub fn sync_window_chrome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let spec = Spec::of(cx);
        let Some(metrics) = self.selected_view().read(cx).metrics else { return };
        let cell = (f32::from(metrics.cell_width), f32::from(metrics.cell_height));
        if self.chrome.window_spec.as_ref() == Some(&(spec.clone(), cell)) {
            return;
        }
        let first = self.chrome.window_spec.is_none();
        self.chrome.window_spec = Some((spec.clone(), cell));
        if first && self.chrome.sized_by_config {
            if let Some(cells) = spec.cells {
                let config = &cx.global::<Theme>().config;
                let padding = (
                    (config.window_padding_x().top_left + config.window_padding_x().bottom_right) as f32,
                    (config.window_padding_y().top_left + config.window_padding_y().bottom_right) as f32,
                );
                let sidebar = if self.sidebar_visible && !spec.tabs_in_titlebar() { self.sidebar_width } else { 0.0 };
                let bar = if !self.sidebar_visible && !spec.tabs_in_titlebar() && self.tabs.len() > 1 { 40.0 } else { 0.0 };
                let (width, height) = content_size(cells, cell, padding, (sidebar, spec.titlebar_height() + bar));
                window.resize(size(px(width), px(height)));
            }
        }
        native::apply(window, &spec, cell);
    }
}

#[cfg(target_os = "macos")]
mod native {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_app_kit::{NSView, NSWindow, NSWindowButton};
    use objc2_foundation::{NSSize, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{Appearance, ColorSpace, Spec};
    use angelly_core::config::MacTitlebarStyle;
    use gpui_kit::Window;

    fn native(window: &Window) -> Option<Retained<NSWindow>> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw() else { return None };
        // SAFETY: the handle comes from a live window on the main thread and holds its NSView.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        view.window()
    }

    pub fn apply(window: &Window, spec: &Spec, cell: (f32, f32)) {
        let Some(native) = native(window) else { return };
        let hidden = spec.style == MacTitlebarStyle::Hidden;
        for button in [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
            if let Some(view) = native.standardWindowButton(button) {
                view.setHidden(hidden);
            }
        }
        let name = match spec.appearance {
            Appearance::System => None,
            Appearance::Light => Some("NSAppearanceNameAqua"),
            Appearance::Dark => Some("NSAppearanceNameDarkAqua"),
        };
        // SAFETY: plain Objective-C calls with matching argument types on the main thread.
        unsafe {
            let appearance: Option<Retained<AnyObject>> =
                name.map(|name| msg_send![class!(NSAppearance), appearanceNamed: &*NSString::from_str(name)]);
            let _: () = msg_send![&*native, setAppearance: appearance.as_deref()];
            let space: Retained<AnyObject> = match spec.color_space {
                ColorSpace::Srgb => msg_send![class!(NSColorSpace), sRGBColorSpace],
                ColorSpace::DisplayP3 => msg_send![class!(NSColorSpace), displayP3ColorSpace],
            };
            let _: () = msg_send![&*native, setColorSpace: &*space];
        }
        let step = if spec.step_resize { NSSize::new(f64::from(cell.0), f64::from(cell.1)) } else { NSSize::new(1.0, 1.0) };
        native.setContentResizeIncrements(step);
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::Spec;
    use gpui_kit::Window;

    pub fn apply(_: &Window, _: &Spec, _: (f32, f32)) {}
}

#[cfg(test)]
mod tests {
    use std::prelude::v1::test;

    use angelly_core::config::{ColorScheme, Env};

    use super::*;

    fn spec(text: &str, dark: bool) -> Spec {
        Spec::from_config(&Config::from_str(text, &Env::from_process(), ColorScheme::Dark), dark)
    }

    #[test]
    fn decoration_none_hides_the_titlebar() {
        assert_eq!(spec("window-decoration = none", true).style, MacTitlebarStyle::Hidden);
        assert_eq!(spec("macos-titlebar-style = tabs", true).style, MacTitlebarStyle::Tabs);
        assert!(spec("macos-titlebar-style = hidden", true).titlebar("x").is_none());
    }

    #[test]
    fn only_the_native_style_has_an_opaque_titlebar() {
        let opaque = |text| !spec(text, true).titlebar("x").unwrap().appears_transparent;
        assert!(opaque("macos-titlebar-style = native"));
        assert!(!opaque("macos-titlebar-style = transparent"));
        assert!(!opaque("macos-titlebar-style = tabs"));
    }

    #[test]
    fn window_theme_follows_the_background_unless_forced() {
        assert_eq!(spec("", true).appearance, Appearance::Dark);
        assert_eq!(spec("", false).appearance, Appearance::Light);
        assert_eq!(spec("window-theme = system", true).appearance, Appearance::System);
        assert_eq!(spec("window-theme = light", true).appearance, Appearance::Light);
    }

    #[test]
    fn fullscreen_and_maximize_choose_the_bounds() {
        let frame = Bounds::new(point(px(0.), px(0.)), size(px(800.), px(600.)));
        assert!(matches!(spec("", true).window_bounds(frame), WindowBounds::Windowed(_)));
        assert!(matches!(spec("maximize = true", true).window_bounds(frame), WindowBounds::Maximized(_)));
        assert!(matches!(spec("fullscreen = true", true).window_bounds(frame), WindowBounds::Fullscreen(_)));
        let non_native = spec("fullscreen = true\nmacos-non-native-fullscreen = true", true);
        assert!(matches!(non_native.window_bounds(frame), WindowBounds::Windowed(_)));
    }

    #[test]
    fn the_initial_size_needs_both_dimensions() {
        assert_eq!(spec("window-width = 100", true).cells, None);
        assert_eq!(spec("window-width = 100\nwindow-height = 30", true).cells, Some((100, 30)));
        assert_eq!(spec("window-width = 3\nwindow-height = 1", true).cells, Some((10, 4)));
    }

    #[test]
    fn content_size_adds_padding_and_chrome() {
        assert_eq!(content_size((80, 24), (8.0, 16.0), (4.0, 4.0), (220.0, 32.0)), (864.0, 420.0));
    }

    #[test]
    fn the_color_space_defaults_to_srgb() {
        assert_eq!(spec("", true).color_space, ColorSpace::Srgb);
        assert_eq!(spec("window-colorspace = display-p3", true).color_space, ColorSpace::DisplayP3);
    }
}
