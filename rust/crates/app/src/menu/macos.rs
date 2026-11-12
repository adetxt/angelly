use std::cell::Cell;

use block2::RcBlock;
use gpui_kit::*;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{define_class, msg_send, sel, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBezierPath, NSColor, NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSView};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{Entry, Icon, Menu, Operation};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "AngellyContextMenuTarget"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<Option<usize>>]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(selectItem:))]
        fn select_item(&self, item: &NSMenuItem) {
            self.ivars().set(usize::try_from(item.tag()).ok());
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(None));
        unsafe { msg_send![super(this), init] }
    }
}

fn swatch(color: Option<Hsla>) -> Retained<NSImage> {
    let rgba: Rgba = color.unwrap_or_else(|| rgb(0x8E8E93).into()).into();
    let drawing = RcBlock::new(move |_: NSRect| {
        let ink = NSColor::colorWithSRGBRed_green_blue_alpha(rgba.r as f64, rgba.g as f64, rgba.b as f64, 1.0);
        let circle = NSBezierPath::bezierPathWithOvalInRect(NSRect::new(NSPoint::new(2., 2.), NSSize::new(12., 12.)));
        if color.is_some() {
            ink.setFill();
            circle.fill();
        } else {
            ink.setStroke();
            circle.setLineWidth(1.);
            circle.stroke();
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(NSSize::new(16., 16.), false, &drawing)
}

fn shortcuts(menu: &Menu, window: &Window, focus: Option<&FocusHandle>, out: &mut Vec<Option<Keystroke>>) {
    for entry in &menu.entries {
        match entry {
            Entry::Command { operation, .. } => {
                let binding = match operation {
                    Operation::Action(action) => {
                        focus.and_then(|focus| window.highest_precedence_binding_for_action_in(action.as_ref(), focus))
                    }
                    Operation::Callback(_) => None,
                };
                out.push(
                    binding
                        .filter(|b| b.keystrokes().len() == 1)
                        .map(|b| b.keystrokes()[0].as_keystroke().clone()),
                );
            }
            Entry::Submenu { menu, .. } => shortcuts(menu, window, focus, out),
            Entry::Separator => {}
        }
    }
}

fn key_equivalent(key: &Keystroke) -> Option<(String, NSEventModifierFlags)> {
    let text = match key.key.as_str() {
        "enter" => "\r",
        "tab" => "\t",
        "space" => " ",
        "backspace" => "\u{7f}",
        "delete" => "\u{f728}",
        "escape" => "\u{1b}",
        "up" => "\u{f700}",
        "down" => "\u{f701}",
        "left" => "\u{f702}",
        "right" => "\u{f703}",
        "home" => "\u{f729}",
        "end" => "\u{f72b}",
        "pageup" => "\u{f72c}",
        "pagedown" => "\u{f72d}",
        text if text.chars().count() == 1 => text,
        _ => "",
    };
    let text = if text.is_empty() {
        let number = key.key.strip_prefix('f')?.parse::<u32>().ok()?;
        if !(1..=35).contains(&number) {
            return None;
        }
        char::from_u32(0xf703 + number)?.to_string()
    } else {
        text.to_string()
    };
    let mut flags = NSEventModifierFlags::empty();
    for (on, flag) in [
        (key.modifiers.platform, NSEventModifierFlags::Command),
        (key.modifiers.control, NSEventModifierFlags::Control),
        (key.modifiers.alt, NSEventModifierFlags::Option),
        (key.modifiers.shift, NSEventModifierFlags::Shift),
        (key.modifiers.function, NSEventModifierFlags::Function),
    ] {
        if on {
            flags |= flag;
        }
    }
    Some((text, flags))
}

fn build(
    menu: Menu,
    target: &MenuTarget,
    operations: &mut Vec<Operation>,
    shortcuts: &mut impl Iterator<Item = Option<Keystroke>>,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let native = NSMenu::new(mtm);
    native.setAutoenablesItems(false);
    for entry in menu.entries {
        let item = match entry {
            Entry::Separator => NSMenuItem::separatorItem(mtm),
            Entry::Submenu { label, menu } => {
                let item = NSMenuItem::new(mtm);
                item.setTitle(&NSString::from_str(&label));
                item.setSubmenu(Some(&build(menu, target, operations, shortcuts, mtm)));
                item
            }
            Entry::Command { label, icon, operation } => {
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &NSString::from_str(&label),
                        Some(sel!(selectItem:)),
                        &NSString::new(),
                    )
                };
                item.setTag(operations.len() as isize);
                unsafe { item.setTarget(Some(target)) };
                if let Some((text, flags)) = shortcuts.next().flatten().as_ref().and_then(key_equivalent) {
                    item.setKeyEquivalent(&NSString::from_str(&text));
                    item.setKeyEquivalentModifierMask(flags);
                }
                operations.push(operation);
                let image = match icon {
                    Some(Icon::Swatch(color)) => Some(swatch(color)),
                    Some(Icon::Image(image)) => {
                        let data = NSData::with_bytes(image.bytes());
                        NSImage::initWithData(NSImage::alloc(), &data).map(|image| {
                            image.setSize(NSSize::new(16., 16.));
                            image
                        })
                    }
                    None => None,
                };
                if let Some(image) = image {
                    image.setTemplate(false);
                    item.setImage(Some(&image));
                    // macOS 27 hides menu images by default; color swatches carry information.
                    if item.respondsToSelector(sel!(setPreferredImageVisibility:)) {
                        unsafe {
                            let _: () = msg_send![&*item, setPreferredImageVisibility: 1_isize];
                        }
                    }
                }
                item
            }
        };
        native.addItem(&item);
    }
    native
}

pub(super) fn show(menu: Menu, position: Point<Pixels>, window: &Window, cx: &App) {
    if menu.entries.is_empty() {
        return;
    }
    let focus = menu.focus.clone().or_else(|| window.focused(cx));
    let mut hints = Vec::new();
    shortcuts(&menu, window, focus.as_ref(), &mut hints);
    let mut async_cx = window.to_async(cx);
    // AppKit runs a nested event loop while tracking. No GPUI borrow may span it.
    cx.foreground_executor()
        .spawn(async move {
            let Ok(Some(view)) = async_cx.update(|window, _| {
                let RawWindowHandle::AppKit(handle) = HasWindowHandle::window_handle(window).ok()?.as_raw() else {
                    return None;
                };
                // The live GPUI window owns this view; retain it while AppKit tracks the menu.
                unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) }
            }) else {
                return;
            };
            let Some(mtm) = MainThreadMarker::new() else { return };
            let target = MenuTarget::new(mtm);
            let mut operations = Vec::new();
            let native = build(menu, &target, &mut operations, &mut hints.into_iter(), mtm);
            let rect = view.bounds();
            let x = rect.origin.x + f64::from(f32::from(position.x));
            let y = if view.isFlipped() {
                rect.origin.y + f64::from(f32::from(position.y))
            } else {
                rect.origin.y + rect.size.height - f64::from(f32::from(position.y))
            };
            if view.window().is_none() {
                return;
            }
            native.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(x, y), Some(&view));
            let selected = target.ivars().get();
            if std::env::var_os("ANGELLY_SELFTEST").is_some() {
                eprintln!("[selftest] native menu selected {selected:?}");
            }
            async_cx
                .update(|window, cx| {
                    if let Some(focus) = focus {
                        window.focus(&focus, cx);
                    }
                    if let Some(operation) = selected.and_then(|index| operations.get(index)) {
                        operation.run(window, cx);
                    }
                })
                .ok();
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test;

    #[test]
    fn native_shortcuts_preserve_modifiers_and_control_keys() {
        let key = Keystroke::parse("ctrl-alt-shift-cmd-c").unwrap();
        let (text, flags) = key_equivalent(&key).unwrap();
        assert_eq!(text, "c");
        assert_eq!(
            flags,
            NSEventModifierFlags::Control
                | NSEventModifierFlags::Option
                | NSEventModifierFlags::Shift
                | NSEventModifierFlags::Command
        );
        assert_eq!(
            key_equivalent(&Keystroke::parse("cmd-up").unwrap()).unwrap().0,
            "\u{f700}"
        );
        assert_eq!(key_equivalent(&Keystroke::parse("f12").unwrap()).unwrap().0, "\u{f70f}");
        assert_eq!(
            key_equivalent(&Keystroke::parse("delete").unwrap()).unwrap().0,
            "\u{f728}"
        );
    }
}
