//! The few system calls the terminal view needs beyond gpui.

use std::process::{Command, Stdio};

#[cfg(target_os = "macos")]
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn NSBeep();
}

/// The system alert sound.
pub fn beep() {
    #[cfg(target_os = "macos")]
    unsafe {
        NSBeep();
    }
}

/// Plays the sound file at `path` at `volume` (0 to 1) without blocking.
pub fn play_audio(path: &str, volume: f32) {
    let path = if let Some(rest) = path.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(home) => std::path::Path::new(&home).join(rest),
            None => return,
        }
    } else {
        path.into()
    };
    let child = Command::new("/usr/bin/afplay")
        .arg("-v")
        .arg(format!("{volume:.2}"))
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

/// Whether the right Option key is held. gpui reports only that Option is
/// down, but `macos-option-as-alt = left|right` needs the side.
pub fn right_option_down() -> bool {
    #[cfg(target_os = "macos")]
    {
        const DEVICE_RIGHT_ALT: usize = 0x40;
        objc2_app_kit::NSEvent::modifierFlags_class().0 & DEVICE_RIGHT_ALT != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Puts plain text and HTML on the system clipboard together, so rich text editors paste the
/// styled version and terminals the plain one. Returns whether the platform supports it.
pub fn write_rich_clipboard(plain: Option<&str>, html: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeString};
        use objc2_foundation::NSString;
        let pasteboard = NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        if let Some(plain) = plain {
            pasteboard.setString_forType(&NSString::from_str(plain), unsafe { NSPasteboardTypeString });
        }
        pasteboard.setString_forType(&NSString::from_str(html), unsafe { NSPasteboardTypeHTML })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (plain, html);
        false
    }
}

/// The virtual key code of the key event being handled. It names the key by position, which
/// the keystroke's layout-dependent `key` does not.
pub fn current_key_code() -> Option<u16> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSEventType};
        use objc2_foundation::MainThreadMarker;
        let event = NSApplication::sharedApplication(MainThreadMarker::new()?).currentEvent()?;
        matches!(event.r#type(), NSEventType::KeyDown | NSEventType::KeyUp).then(|| event.keyCode())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
