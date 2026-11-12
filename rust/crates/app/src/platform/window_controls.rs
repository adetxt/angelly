//! Minimize, maximize and close buttons for windows whose frame the app draws itself:
//! Linux windows with client-side decorations (Wayland compositors without server-side
//! decorations) and every Windows window, where the app owns the caption area.

use gpui_kit::component::{Icon, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::menus::CloseWindow;
use crate::sidebar::Palette;

const BUTTON_WIDTH: f32 = 36.0;

/// Whether the app draws the window buttons and moves the window itself.
pub fn drawn(window: &Window) -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && matches!(window.window_decorations(), Decorations::Client { .. }))
}

/// Which buttons the window manager lets the app offer. Windows always offers all three.
fn supported(window: &Window) -> WindowControls {
    if cfg!(windows) {
        WindowControls { fullscreen: true, maximize: true, minimize: true, window_menu: false }
    } else {
        window.window_controls()
    }
}

/// Width the buttons take at the trailing edge of the titlebar.
pub fn width(window: &Window) -> f32 {
    if !drawn(window) {
        return 0.0;
    }
    let supported = supported(window);
    BUTTON_WIDTH * (1.0 + f32::from(u8::from(supported.minimize)) + f32::from(u8::from(supported.maximize)))
}

/// Moves a client-decorated window when its titlebar is dragged, and zooms it on a double click.
pub fn drag_area<E: InteractiveElement>(element: E, window: &Window) -> E {
    if !cfg!(target_os = "linux") || !drawn(window) {
        return element;
    }
    element.on_mouse_down(MouseButton::Left, |event, window, _| {
        if event.click_count >= 2 {
            window.zoom_window();
        } else {
            window.start_window_move();
        }
    })
}

#[derive(Clone, Copy)]
enum Button {
    Minimize,
    Maximize,
    Restore,
    Close,
}

impl Button {
    fn id(self) -> &'static str {
        match self {
            Button::Minimize => "window-minimize",
            Button::Maximize => "window-maximize",
            Button::Restore => "window-restore",
            Button::Close => "window-close",
        }
    }

    fn icon(self) -> IconName {
        match self {
            Button::Minimize => IconName::WindowMinimize,
            Button::Maximize => IconName::WindowMaximize,
            Button::Restore => IconName::WindowRestore,
            Button::Close => IconName::WindowClose,
        }
    }

    fn area(self) -> WindowControlArea {
        match self {
            Button::Minimize => WindowControlArea::Min,
            Button::Maximize | Button::Restore => WindowControlArea::Max,
            Button::Close => WindowControlArea::Close,
        }
    }

    fn press(self, window: &mut Window, cx: &mut App) {
        match self {
            Button::Minimize => window.minimize_window(),
            Button::Maximize | Button::Restore => window.zoom_window(),
            Button::Close => window.dispatch_action(Box::new(CloseWindow), cx),
        }
    }

    fn element(self, palette: Palette, height: f32) -> impl IntoElement {
        let close = matches!(self, Button::Close);
        let hover = if close { hsla(0.0, 0.72, 0.5, 1.0) } else { palette.tint(0.1) };
        let hover_text = if close { white() } else { palette.primary };
        div()
            .id(self.id())
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .w(px(BUTTON_WIDTH))
            .h(px(height))
            .text_color(palette.secondary)
            .hover(|style| style.bg(hover).text_color(hover_text))
            .when(cfg!(windows), |element| element.window_control_area(self.area()))
            .when(cfg!(target_os = "linux"), |element| {
                element
                    .on_mouse_down(MouseButton::Left, |_, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    })
                    .on_click(move |_, window, cx| {
                        cx.stop_propagation();
                        self.press(window, cx);
                    })
            })
            .child(Icon::new(self.icon()).small())
    }
}

/// The buttons, or `None` when the system draws them.
pub fn render(window: &Window, palette: Palette, height: f32) -> Option<impl IntoElement> {
    if !drawn(window) {
        return None;
    }
    let supported = supported(window);
    Some(
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(height))
            .flex_none()
            .when(supported.minimize, |row| row.child(Button::Minimize.element(palette, height)))
            .when(supported.maximize, |row| {
                row.child(if window.is_maximized() { Button::Restore } else { Button::Maximize }.element(palette, height))
            })
            .child(Button::Close.element(palette, height)),
    )
}
