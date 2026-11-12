//! The native window behind the quick terminal: a borderless, non-activating panel that
//! GPUI creates and this module positions, layers and shows without GPUI's help.

use gpui_kit::Window;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSEvent, NSFloatingWindowLevel, NSPopUpMenuWindowLevel, NSRunningApplication,
    NSScreen, NSUserInterfaceItemIdentification, NSView, NSWindow, NSWindowCollectionBehavior, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSProcessInfo, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::geometry::Rect;

const IDENTIFIER: &str = "com.adetxt.angelly.quickTerminal";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Space {
    /// The panel follows the active space.
    #[default]
    Move,
    /// The panel stays on the space it was shown in.
    Remain,
}

impl Space {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "move" => Some(Self::Move),
            "remain" => Some(Self::Remain),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenChoice {
    #[default]
    Main,
    Mouse,
    MenuBar,
}

impl ScreenChoice {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "main" => Some(Self::Main),
            "mouse" => Some(Self::Mouse),
            "macos-menu-bar" => Some(Self::MenuBar),
            _ => None,
        }
    }
}

fn rect(frame: NSRect) -> Rect {
    Rect::new(frame.origin.x, frame.origin.y, frame.size.width, frame.size.height)
}

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(NSPoint::new(rect.x, rect.y), NSSize::new(rect.width, rect.height))
}

/// The usable area of the chosen screen.
pub fn screen_area(choice: ScreenChoice) -> Option<Rect> {
    let mtm = MainThreadMarker::new()?;
    let screen = match choice {
        ScreenChoice::Main => NSScreen::mainScreen(mtm),
        ScreenChoice::Mouse => {
            let mouse = NSEvent::mouseLocation();
            let screens = NSScreen::screens(mtm);
            screens.iter().find(|screen| {
                let frame = screen.frame();
                mouse.x >= frame.origin.x
                    && mouse.x < frame.origin.x + frame.size.width
                    && mouse.y >= frame.origin.y
                    && mouse.y < frame.origin.y + frame.size.height
            })
        }
        ScreenChoice::MenuBar => NSScreen::screens(mtm).iter().next(),
    }
    .or_else(|| NSScreen::mainScreen(mtm))?;
    Some(rect(screen.visibleFrame()))
}

pub struct Panel(Retained<NSWindow>);

impl Panel {
    /// The native window of a GPUI window.
    pub fn of(window: &Window) -> Option<Self> {
        let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw() else { return None };
        // SAFETY: the handle comes from a live window on the main thread and holds its NSView.
        let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        view.window().map(Self)
    }

    pub fn configure(&self, space: Space) {
        self.0.setIdentifier(Some(&NSString::from_str(IDENTIFIER)));
        self.0.setHidesOnDeactivate(false);
        let base = NSWindowCollectionBehavior::IgnoresCycle | NSWindowCollectionBehavior::FullScreenAuxiliary;
        self.0.setCollectionBehavior(match space {
            Space::Move => base | NSWindowCollectionBehavior::CanJoinAllSpaces,
            Space::Remain => base | NSWindowCollectionBehavior::MoveToActiveSpace,
        });
    }

    pub fn frame(&self) -> Rect {
        rect(self.0.frame())
    }

    pub fn set_frame(&self, frame: Rect, alpha: f64) {
        self.0.setAlphaValue(alpha);
        self.0.setFrame_display(ns_rect(frame), true);
    }

    /// Orders the panel in above menus so it can slide over the menu bar, then makes it key.
    pub fn show(&self) {
        self.0.setLevel(NSPopUpMenuWindowLevel);
        self.0.makeKeyAndOrderFront(None);
    }

    /// Settles into the level the panel keeps once its animation has finished.
    pub fn settle(&self) {
        self.0.setLevel(NSFloatingWindowLevel);
    }

    pub fn hide(&self) {
        self.0.orderOut(None);
    }

    pub fn is_visible(&self) -> bool {
        self.0.isVisible()
    }
}

/// The application that was frontmost before the quick terminal appeared, so focus can
/// go back to it afterwards.
pub struct PreviousApp(Retained<NSRunningApplication>);

impl PreviousApp {
    pub fn capture() -> Option<Self> {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let own = NSProcessInfo::processInfo().processIdentifier();
        (app.processIdentifier() != own).then_some(Self(app))
    }

    pub fn restore(self) {
        if !self.0.isTerminated() {
            self.0.activateWithOptions(NSApplicationActivationOptions::empty());
        }
    }
}
