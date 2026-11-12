//! The C interface the Swift shim calls (see `shim/include/angelly_bridge.h`).
//!
//! Every entry runs on the main thread, outside any GPUI update, and must not unwind
//! into Swift, so panics become error responses.

use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use angelly_core::config::parse_bool;
use gpui_kit::App;
use serde_json::json;

use super::{automation, command, runtime, state};
use crate::theme::Theme;

fn into_c(text: String) -> *mut c_char {
    CString::new(text.replace('\0', "")).map_or(std::ptr::null_mut(), CString::into_raw)
}

fn with_host<R>(f: impl FnOnce(&dyn command::Host, &mut App) -> R) -> Option<R> {
    catch_unwind(AssertUnwindSafe(|| {
        runtime::with_app(|cx| {
            let host = command::host(cx)?;
            Some(f(&*host, cx))
        })
        .flatten()
    }))
    .ok()
    .flatten()
}

fn config_value(key: &str) -> Option<String> {
    runtime::with_app(|cx| cx.try_global::<Theme>().and_then(|t| t.config.get(key).map(str::to_owned))).flatten()
}

pub fn applescript_enabled() -> bool {
    config_value("macos-applescript").and_then(|v| parse_bool(&v)).unwrap_or(true)
}

/// The `macos-shortcuts` setting: 0 ask, 1 allow, 2 deny.
pub fn shortcuts_policy() -> i32 {
    match config_value("macos-shortcuts").as_deref() {
        Some("allow") => 1,
        Some("deny") => 2,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn angelly_automation_snapshot() -> *mut c_char {
    into_c(with_host(|host, cx| automation::snapshot_json(host, cx)).unwrap_or_else(|| "{}".into()))
}

/// # Safety
/// `json` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn angelly_automation_request(json: *const c_char) -> *mut c_char {
    if json.is_null() {
        return into_c(json!({ "ok": false, "error": "Missing request." }).to_string());
    }
    let request = unsafe { CStr::from_ptr(json) }.to_string_lossy().into_owned();
    let response = with_host(|host, cx| automation::handle_json(host, &request, cx))
        .unwrap_or_else(|| json!({ "ok": false, "error": "Angelly is unavailable." }).to_string());
    into_c(response)
}

/// # Safety
/// `string` must come from one of this module's functions and not be freed twice.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn angelly_string_free(string: *mut c_char) {
    if !string.is_null() {
        drop(unsafe { CString::from_raw(string) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn angelly_applescript_enabled() -> bool {
    applescript_enabled()
}

#[unsafe(no_mangle)]
pub extern "C" fn angelly_shortcuts_policy() -> i32 {
    shortcuts_policy()
}

#[unsafe(no_mangle)]
pub extern "C" fn angelly_shortcuts_remembered() -> i32 {
    state::get_bool(state::SHORTCUTS_PERMISSION).map_or(-1, i32::from)
}

#[unsafe(no_mangle)]
pub extern "C" fn angelly_shortcuts_remember(allowed: bool) {
    state::set(state::SHORTCUTS_PERMISSION, allowed);
}
