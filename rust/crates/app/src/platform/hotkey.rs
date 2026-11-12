//! Global keybinds (spec 16.4): `keybind = global:...` entries fire while another app
//! is frontmost. On macOS they use a CGEvent tap, which needs the Accessibility
//! permission; the permission is requested once and polled until it is granted.

use angelly_core::config::{Binding, Config, Mods, Trigger, TriggerKey};

/// A global binding reduced to what the event tap compares against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalBinding {
    pub trigger: Trigger,
    pub action: String,
}

pub fn global_bindings(config: &Config) -> Vec<GlobalBinding> {
    config
        .keybinds()
        .set
        .iter()
        .filter(|binding: &&Binding| binding.flags.global && !binding.is_sequence())
        .map(|binding| GlobalBinding {
            trigger: binding.trigger().clone(),
            action: match &binding.action.param {
                Some(param) => format!("{}:{param}", binding.action.name),
                None => binding.action.name.clone(),
            },
        })
        .collect()
}

/// macOS virtual key code of a W3C key name on an ANSI keyboard.
pub fn key_code(name: &str) -> Option<u16> {
    let code = match name {
        "key_a" => 0,
        "key_s" => 1,
        "key_d" => 2,
        "key_f" => 3,
        "key_h" => 4,
        "key_g" => 5,
        "key_z" => 6,
        "key_x" => 7,
        "key_c" => 8,
        "key_v" => 9,
        "intl_backslash" => 10,
        "key_b" => 11,
        "key_q" => 12,
        "key_w" => 13,
        "key_e" => 14,
        "key_r" => 15,
        "key_y" => 16,
        "key_t" => 17,
        "digit_1" => 18,
        "digit_2" => 19,
        "digit_3" => 20,
        "digit_4" => 21,
        "digit_6" => 22,
        "digit_5" => 23,
        "equal" => 24,
        "digit_9" => 25,
        "digit_7" => 26,
        "minus" => 27,
        "digit_8" => 28,
        "digit_0" => 29,
        "bracket_right" => 30,
        "key_o" => 31,
        "key_u" => 32,
        "bracket_left" => 33,
        "key_i" => 34,
        "key_p" => 35,
        "enter" => 36,
        "key_l" => 37,
        "key_j" => 38,
        "quote" => 39,
        "key_k" => 40,
        "semicolon" => 41,
        "backslash" => 42,
        "comma" => 43,
        "slash" => 44,
        "key_n" => 45,
        "key_m" => 46,
        "period" => 47,
        "tab" => 48,
        "space" => 49,
        "backquote" => 50,
        "backspace" => 51,
        "escape" => 53,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "home" => 115,
        "page_up" => 116,
        "delete" => 117,
        "end" => 119,
        "page_down" => 121,
        "arrow_left" => 123,
        "arrow_right" => 124,
        "arrow_down" => 125,
        "arrow_up" => 126,
        _ => return None,
    };
    Some(code)
}

/// The key code of the key that types `ch` on an ANSI layout, used for triggers
/// written as characters.
fn key_code_of_char(ch: char) -> Option<u16> {
    let name = match ch.to_ascii_lowercase() {
        c @ 'a'..='z' => format!("key_{c}"),
        c @ '0'..='9' => format!("digit_{c}"),
        '`' => "backquote".into(),
        '-' => "minus".into(),
        '=' => "equal".into(),
        '[' => "bracket_left".into(),
        ']' => "bracket_right".into(),
        '\\' => "backslash".into(),
        ';' => "semicolon".into(),
        '\'' => "quote".into(),
        ',' => "comma".into(),
        '.' => "period".into(),
        '/' => "slash".into(),
        ' ' => "space".into(),
        _ => return None,
    };
    key_code(&name)
}

/// A key press as the event tap sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    pub key_code: u16,
    pub characters: String,
    pub mods: Mods,
}

pub fn matches(binding: &GlobalBinding, press: &KeyPress) -> bool {
    if binding.trigger.mods != press.mods {
        return false;
    }
    match &binding.trigger.key {
        TriggerKey::Physical(name) => key_code(name) == Some(press.key_code),
        TriggerKey::Unicode(ch) => {
            key_code_of_char(*ch) == Some(press.key_code)
                || press.characters.chars().next().is_some_and(|c| c.to_lowercase().eq(ch.to_lowercase()))
        }
        TriggerKey::CatchAll => false,
    }
}

pub fn find<'a>(bindings: &'a [GlobalBinding], press: &KeyPress) -> Option<&'a GlobalBinding> {
    bindings.iter().find(|binding| matches(binding, press))
}

#[cfg(target_os = "macos")]
pub use tap::reload;

#[cfg(all(unix, not(target_os = "macos")))]
pub use portal::reload;

#[cfg(windows)]
pub fn reload(_: &mut gpui_kit::App) {}

/// The trigger in the XDG shortcuts syntax the GlobalShortcuts portal takes, for example
/// `CTRL+SHIFT+grave`. Key names are X11 keysym names. `None` for keys without one.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn portal_trigger(trigger: &Trigger) -> Option<String> {
    let key = match &trigger.key {
        TriggerKey::Unicode(c) => keysym_of_char(*c)?,
        TriggerKey::Physical(name) => keysym_of_physical(name)?,
        TriggerKey::CatchAll => return None,
    };
    let mut parts: Vec<&str> = Vec::new();
    if trigger.mods.ctrl {
        parts.push("CTRL");
    }
    if trigger.mods.alt {
        parts.push("ALT");
    }
    if trigger.mods.shift {
        parts.push("SHIFT");
    }
    if trigger.mods.super_key {
        parts.push("LOGO");
    }
    parts.push(&key);
    Some(parts.join("+"))
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn keysym_of_char(c: char) -> Option<String> {
    let name = match c {
        'a'..='z' | '0'..='9' => return Some(c.to_string()),
        'A'..='Z' => return Some(c.to_ascii_lowercase().to_string()),
        '`' => "grave",
        '-' => "minus",
        '=' => "equal",
        '[' => "bracketleft",
        ']' => "bracketright",
        '\\' => "backslash",
        ';' => "semicolon",
        '\'' => "apostrophe",
        ',' => "comma",
        '.' => "period",
        '/' => "slash",
        ' ' => "space",
        _ => return None,
    };
    Some(name.to_owned())
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn keysym_of_physical(name: &str) -> Option<String> {
    if let Some(letter) = name.strip_prefix("key_").filter(|rest| rest.len() == 1) {
        return Some(letter.to_owned());
    }
    if let Some(digit) = name.strip_prefix("digit_").filter(|rest| rest.len() == 1) {
        return Some(digit.to_owned());
    }
    if let Some(number) = name.strip_prefix('f').filter(|rest| rest.parse::<u8>().is_ok_and(|n| (1..=24).contains(&n))) {
        return Some(format!("F{number}"));
    }
    let keysym = match name {
        "backquote" => "grave",
        "minus" => "minus",
        "equal" => "equal",
        "bracket_left" => "bracketleft",
        "bracket_right" => "bracketright",
        "backslash" => "backslash",
        "semicolon" => "semicolon",
        "quote" => "apostrophe",
        "comma" => "comma",
        "period" => "period",
        "slash" => "slash",
        "space" => "space",
        "enter" => "Return",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "escape" => "Escape",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "page_up" => "Prior",
        "page_down" => "Next",
        "arrow_left" => "Left",
        "arrow_right" => "Right",
        "arrow_up" => "Up",
        "arrow_down" => "Down",
        _ => return None,
    };
    Some(keysym.to_owned())
}

/// Global keybinds through the `org.freedesktop.portal.GlobalShortcuts` portal. The desktop
/// asks the user to confirm the bindings once; compositors without the portal (anything
/// but recent GNOME and KDE) leave the global keybinds inactive.
#[cfg(all(unix, not(target_os = "macos")))]
mod portal {
    use std::collections::HashMap;

    use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
    use ashpd::desktop::CreateSessionOptions;
    use futures_util::StreamExt as _;
    use gpui_kit::{App, AsyncApp, Global, Task};

    use super::{global_bindings, portal_trigger};
    use crate::platform::actions;
    use crate::theme::Theme;

    struct Session {
        _task: Task<()>,
    }

    impl Global for Session {}

    pub fn reload(cx: &mut App) {
        let bindings: Vec<(String, String, String)> = global_bindings(&cx.global::<Theme>().config)
            .into_iter()
            .filter_map(|binding| {
                let trigger = portal_trigger(&binding.trigger)?;
                Some((binding.action.clone(), binding.action, trigger))
            })
            .collect();
        if bindings.is_empty() {
            if cx.has_global::<Session>() {
                cx.remove_global::<Session>();
            }
            return;
        }
        let task = cx.spawn(async move |cx| {
            if let Err(error) = run(&bindings, cx).await {
                eprintln!("angelly: global shortcuts unavailable: {error}");
            }
        });
        cx.set_global(Session { _task: task });
    }

    async fn run(bindings: &[(String, String, String)], cx: &mut AsyncApp) -> ashpd::Result<()> {
        let proxy = GlobalShortcuts::new().await?;
        let session = proxy.create_session(CreateSessionOptions::default()).await?;
        let mut actions_by_id = HashMap::new();
        let mut shortcuts = Vec::new();
        for (index, (action, description, trigger)) in bindings.iter().enumerate() {
            let id = format!("global-{index}");
            shortcuts.push(NewShortcut::new(id.clone(), description.as_str()).preferred_trigger(trigger.as_str()));
            actions_by_id.insert(id, action.clone());
        }
        proxy.bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default()).await?;
        let mut activated = proxy.receive_activated().await?;
        while let Some(event) = activated.next().await {
            if let Some(action) = actions_by_id.get(event.shortcut_id()) {
                cx.update(|cx| actions::perform_global(action, cx));
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod tap {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicPtr, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use angelly_core::config::Mods;
    use gpui_kit::{App, Global, Task};
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSEvent};
    use objc2_foundation::{MainThreadMarker, NSString};

    use super::{find, global_bindings, GlobalBinding, KeyPress};
    use crate::platform::actions;
    use crate::theme::Theme;

    const SESSION_TAP: u32 = 1;
    const HEAD_INSERT: u32 = 0;
    const OPTION_DEFAULT: u32 = 0;
    const KEY_DOWN: u32 = 10;
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER: u32 = 0xFFFF_FFFF;
    const KEYBOARD_KEYCODE_FIELD: u32 = 9;
    const FLAG_SHIFT: u64 = 0x0002_0000;
    const FLAG_CONTROL: u64 = 0x0004_0000;
    const FLAG_ALTERNATE: u64 = 0x0008_0000;
    const FLAG_COMMAND: u64 = 0x0010_0000;

    type TapCallback = extern "C" fn(*mut c_void, u32, *mut c_void, *mut c_void) -> *mut c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: TapCallback,
            user_info: *mut c_void,
        ) -> *mut c_void;
        fn CGEventTapEnable(tap: *mut c_void, enable: bool);
        fn CGEventGetFlags(event: *mut c_void) -> u64;
        fn CGEventGetIntegerValueField(event: *mut c_void, field: u32) -> i64;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: *const c_void;
        static kCFBooleanTrue: *const c_void;
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        fn CFMachPortCreateRunLoopSource(allocator: *const c_void, port: *mut c_void, order: isize) -> *mut c_void;
        fn CFMachPortInvalidate(port: *mut c_void);
        fn CFRunLoopAddSource(run_loop: *mut c_void, source: *mut c_void, mode: *const c_void);
        fn CFRunLoopGetMain() -> *mut c_void;
        fn CFRelease(object: *const c_void);
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            count: isize,
            key_callbacks: *const u8,
            value_callbacks: *const u8,
        ) -> *const c_void;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        static kAXTrustedCheckOptionPrompt: *const c_void;
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    static BINDINGS: Mutex<Vec<GlobalBinding>> = Mutex::new(Vec::new());
    static TAP: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
    static CHANNEL: OnceLock<(async_channel::Sender<String>, async_channel::Receiver<String>)> = OnceLock::new();

    fn channel() -> &'static (async_channel::Sender<String>, async_channel::Receiver<String>) {
        CHANNEL.get_or_init(async_channel::unbounded)
    }

    /// Keeps the permission poll and the action consumer alive until the next reload.
    struct Tasks {
        _poll: Option<Task<()>>,
        _consumer: Task<()>,
    }

    impl Global for Tasks {}

    fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() }
    }

    /// Asks macOS to show the Accessibility prompt. Skipped under the self-test flag so
    /// automated runs never raise a system dialog.
    fn prompt_for_access() {
        if std::env::var_os("ANGELLY_SELFTEST").is_some() {
            return;
        }
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(
                ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            AXIsProcessTrustedWithOptions(options);
            CFRelease(options);
        }
    }

    fn mods_of(flags: u64) -> Mods {
        Mods {
            shift: flags & FLAG_SHIFT != 0,
            ctrl: flags & FLAG_CONTROL != 0,
            alt: flags & FLAG_ALTERNATE != 0,
            super_key: flags & FLAG_COMMAND != 0,
        }
    }

    fn characters_of(event: *mut c_void) -> String {
        unsafe {
            let ns: Option<Retained<NSEvent>> = msg_send![objc2::class!(NSEvent), eventWithCGEvent: event];
            ns.and_then(|ns| ns.charactersIgnoringModifiers()).map(|s: Retained<NSString>| s.to_string()).unwrap_or_default()
        }
    }

    fn app_is_active() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| NSApplication::sharedApplication(mtm).isActive())
    }

    extern "C" fn on_event(_: *mut c_void, kind: u32, event: *mut c_void, _: *mut c_void) -> *mut c_void {
        if kind == TAP_DISABLED_BY_TIMEOUT || kind == TAP_DISABLED_BY_USER {
            let tap = TAP.load(Ordering::Acquire);
            if !tap.is_null() {
                unsafe { CGEventTapEnable(tap, true) };
            }
            return event;
        }
        if kind != KEY_DOWN || app_is_active() {
            return event;
        }
        let press = unsafe {
            KeyPress {
                key_code: CGEventGetIntegerValueField(event, KEYBOARD_KEYCODE_FIELD) as u16,
                characters: characters_of(event),
                mods: mods_of(CGEventGetFlags(event)),
            }
        };
        let action = {
            let bindings = BINDINGS.lock().unwrap_or_else(|e| e.into_inner());
            find(&bindings, &press).map(|binding| binding.action.clone())
        };
        match action {
            Some(action) => {
                let _ = channel().0.try_send(action);
                ptr::null_mut()
            }
            None => event,
        }
    }

    fn try_create_tap() -> bool {
        if !TAP.load(Ordering::Acquire).is_null() {
            return true;
        }
        unsafe {
            let tap = CGEventTapCreate(SESSION_TAP, HEAD_INSERT, OPTION_DEFAULT, 1 << KEY_DOWN, on_event, ptr::null_mut());
            if tap.is_null() {
                return false;
            }
            let source = CFMachPortCreateRunLoopSource(ptr::null(), tap, 0);
            CFRunLoopAddSource(CFRunLoopGetMain(), source, kCFRunLoopCommonModes);
            TAP.store(tap, Ordering::Release);
        }
        true
    }

    fn disable_tap() {
        let tap = TAP.swap(ptr::null_mut(), Ordering::AcqRel);
        if !tap.is_null() {
            unsafe { CFMachPortInvalidate(tap) };
        }
    }

    /// Reads the global bindings from the loaded config and (de)activates the tap.
    pub fn reload(cx: &mut App) {
        let bindings = global_bindings(&cx.global::<Theme>().config);
        let any = !bindings.is_empty();
        *BINDINGS.lock().unwrap_or_else(|e| e.into_inner()) = bindings;
        if !any {
            disable_tap();
            if cx.has_global::<Tasks>() {
                cx.remove_global::<Tasks>();
            }
            return;
        }

        let receiver = channel().1.clone();
        let consumer = cx.spawn(async move |cx| {
            while let Ok(action) = receiver.recv().await {
                cx.update(|cx| actions::perform_global(&action, cx));
            }
        });

        // Creating a tap without the permission leaks a Mach port each attempt, so create
        // it only once the cheap trust check passes and poll that check until it does.
        let poll = if trusted() {
            try_create_tap();
            None
        } else if TAP.load(Ordering::Acquire).is_null() {
            prompt_for_access();
            Some(cx.spawn(async move |cx| loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if trusted() {
                    try_create_tap();
                    break;
                }
            }))
        } else {
            None
        };
        cx.set_global(Tasks { _poll: poll, _consumer: consumer });
    }
}

#[cfg(test)]
mod tests {
    use angelly_core::config::{ColorScheme, Env, Platform};

    use super::*;

    fn config(text: &str) -> Config {
        let env = Env { platform: Platform::MacOs, home: None, xdg_config_home: None, resources_dir: None };
        Config::from_str(text, &env, ColorScheme::Dark)
    }

    fn press(key_code: u16, characters: &str, mods: Mods) -> KeyPress {
        KeyPress { key_code, characters: characters.into(), mods }
    }

    #[test]
    fn only_global_single_key_bindings_are_collected() {
        let config = config(
            "keybind = global:ctrl+grave_accent=toggle_quick_terminal\n\
             keybind = cmd+j=new_window\n\
             keybind = global:cmd+shift+space=toggle_visibility\n\
             keybind = global:ctrl+a>b=new_window\n",
        );
        let bindings = global_bindings(&config);
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].action, "toggle_quick_terminal");
        assert_eq!(bindings[1].action, "toggle_visibility");
    }

    #[test]
    fn a_character_trigger_matches_its_key_and_exact_modifiers() {
        let config = config("keybind = global:ctrl+grave_accent=toggle_quick_terminal\n");
        let bindings = global_bindings(&config);
        let ctrl = Mods::NONE.ctrl();
        assert!(find(&bindings, &press(50, "`", ctrl)).is_some());
        assert!(find(&bindings, &press(50, "`", ctrl.shift())).is_none());
        assert!(find(&bindings, &press(49, " ", ctrl)).is_none());
    }

    #[test]
    fn triggers_become_portal_shortcuts() {
        let config = config(
            "keybind = global:ctrl+grave_accent=toggle_quick_terminal\n\
             keybind = global:super+shift+arrow_left=toggle_visibility\n\
             keybind = global:alt+f5=new_window\n",
        );
        let triggers: Vec<_> = global_bindings(&config).iter().map(|b| portal_trigger(&b.trigger)).collect();
        assert_eq!(triggers[0].as_deref(), Some("CTRL+grave"));
        assert_eq!(triggers[1].as_deref(), Some("SHIFT+LOGO+Left"));
        assert_eq!(triggers[2].as_deref(), Some("ALT+F5"));
    }

    #[test]
    fn a_physical_trigger_matches_by_key_code() {
        let config = config("keybind = global:super+shift+arrow_left=toggle_visibility\n");
        let bindings = global_bindings(&config);
        assert!(find(&bindings, &press(123, "", Mods::NONE.super_key().shift())).is_some());
        assert!(find(&bindings, &press(124, "", Mods::NONE.super_key().shift())).is_none());
    }
}
