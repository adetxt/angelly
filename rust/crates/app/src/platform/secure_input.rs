//! Secure Keyboard Entry (spec 16.3): Carbon `EnableSecureEventInput`, held only while
//! the app is active and persisted as the `SecureInput` switch.
//!
//! The state is global and every enable must be balanced by a disable, so the
//! bookkeeping lives here instead of in the views. Besides the user's switch, a focused
//! surface showing a password prompt turns it on for as long as the prompt lasts. The indicator overlay reads
//! [`is_enabled`].

use gpui_kit::Global;

pub use imp::{init, is_enabled, request, toggle};

/// Registered so views can observe changes with `cx.observe_global`.
pub struct SecureInputState;

impl Global for SecureInputState {}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use gpui_kit::App;
    use objc2_app_kit::{NSApplication, NSApplicationDidBecomeActiveNotification, NSApplicationDidResignActiveNotification};
    use objc2_foundation::MainThreadMarker;

    use super::super::notifications::{observe, Observer};
    use super::super::state;
    use super::SecureInputState;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        fn EnableSecureEventInput() -> i32;
        fn DisableSecureEventInput() -> i32;
    }

    static GLOBAL: AtomicBool = AtomicBool::new(false);
    static ENABLED: AtomicBool = AtomicBool::new(false);
    /// Surfaces that currently ask for secure input.
    static REQUESTS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

    fn app_is_active() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive())
    }

    /// Brings the system state in line with the desired one. While the app is inactive
    /// the switch stays released; the activation observer reapplies it.
    fn apply() {
        if !app_is_active() {
            return;
        }
        let want = GLOBAL.load(Ordering::Relaxed) || !REQUESTS.lock().unwrap().is_empty();
        if ENABLED.load(Ordering::Relaxed) == want {
            return;
        }
        let status = unsafe {
            if want {
                EnableSecureEventInput()
            } else {
                DisableSecureEventInput()
            }
        };
        if status == 0 {
            ENABLED.store(want, Ordering::Relaxed);
        }
    }

    fn release() {
        if ENABLED.swap(false, Ordering::Relaxed) {
            unsafe { DisableSecureEventInput() };
        }
    }

    struct Observers(#[allow(dead_code)] [Observer; 2]);

    pub fn init(cx: &mut App) {
        cx.set_global(SecureInputState);
        GLOBAL.store(state::get_bool(state::SECURE_INPUT).unwrap_or(false), Ordering::Relaxed);
        let resign = observe(unsafe { NSApplicationDidResignActiveNotification }, release);
        let become_active = observe(unsafe { NSApplicationDidBecomeActiveNotification }, apply);
        // Observers live as long as the process; keep them out of any drop path.
        std::mem::forget(Observers([resign, become_active]));
        apply();
    }

    /// Whether secure input is actually on right now.
    pub fn is_enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    /// The user-facing switch behind the "Secure Keyboard Entry" menu item.
    pub fn toggle(cx: &mut App) {
        let on = !GLOBAL.load(Ordering::Relaxed);
        GLOBAL.store(on, Ordering::Relaxed);
        state::set(state::SECURE_INPUT, on);
        apply();
        cx.set_global(SecureInputState);
    }

    /// Whether the surface `owner` wants secure input right now. Idempotent.
    pub fn request(owner: u64, wanted: bool, cx: &mut App) {
        {
            let mut requests = REQUESTS.lock().unwrap();
            let present = requests.contains(&owner);
            if present == wanted {
                return;
            }
            if wanted {
                requests.push(owner);
            } else {
                requests.retain(|o| *o != owner);
            }
        }
        apply();
        cx.set_global(SecureInputState);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use gpui_kit::App;

    pub fn init(_: &mut App) {}

    pub fn is_enabled() -> bool {
        false
    }

    pub fn toggle(_: &mut App) {}

    pub fn request(_: u64, _: bool, _: &mut App) {}
}
