//! Native popup material, matching AppKit menus and respecting older macOS releases.

use gpui_kit::Window;

pub fn install(window: &Window, parent: &Window, dark: bool) {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc2::{msg_send, rc::Retained, runtime::AnyClass, MainThreadMarker, MainThreadOnly};
        use objc2_app_kit::{
            NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua,
            NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions, NSView,
            NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
            NSVisualEffectView, NSWindow, NSWindowOrderingMode, NSWindowStyleMask,
        };
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        let native = |window: &Window| -> Option<Retained<NSWindow>> {
            let RawWindowHandle::AppKit(handle) =
                HasWindowHandle::window_handle(window).ok()?.as_raw()
            else {
                return None;
            };
            let view = &*handle.ns_view.as_ptr().cast::<NSView>();
            view.window()
        };
        let (Some(popup), Some(parent)) = (native(window), native(parent)) else {
            return;
        };
        let Some(content) = popup.contentView() else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        popup.setStyleMask(NSWindowStyleMask::NonactivatingPanel);
        popup.setHasShadow(true);
        popup.setAppearance(
            NSAppearance::appearanceNamed(if dark {
                NSAppearanceNameDarkAqua
            } else {
                NSAppearanceNameAqua
            })
            .as_deref(),
        );
        let frame = content.frame();
        let material: Retained<NSView> = if let Some(class) = AnyClass::get(c"NSGlassEffectView") {
            let glass: Retained<NSView> = msg_send![class, new];
            glass.setFrame(frame);
            let _: () = msg_send![&*glass, setCornerRadius: 10.0f64];
            let _: () = msg_send![&*glass, setContentView: &*content];
            glass
        } else {
            let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
            effect.setMaterial(NSVisualEffectMaterial::Menu);
            effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect.setState(NSVisualEffectState::Active);
            effect.addSubview(&content);
            Retained::into_super(effect)
        };
        content.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        popup.setContentView(Some(&material));
        parent.addChildWindow_ordered(&popup, NSWindowOrderingMode::Above);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, parent, dark);
}
