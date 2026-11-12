use std::{mem::size_of, slice};

use vt_sys as sys;

pub(crate) const OK: sys::GhosttyResult::Type = sys::GhosttyResult::GHOSTTY_SUCCESS;

pub(crate) fn check(r: sys::GhosttyResult::Type, what: &str) {
    assert_eq!(r, OK, "{what} failed with code {r}");
}

pub(crate) fn sized<T: Default>(set_size: impl FnOnce(&mut T, usize)) -> T {
    let mut v = T::default();
    set_size(&mut v, size_of::<T>());
    v
}

/// # Safety
/// `s` must point to `len` readable bytes (or be empty).
pub(crate) unsafe fn string(s: sys::GhosttyString) -> String {
    if s.ptr.is_null() || s.len == 0 {
        return String::new();
    }
    String::from_utf8_lossy(slice::from_raw_parts(s.ptr, s.len)).into_owned()
}

pub(crate) fn terminal_get<T: Default>(
    term: sys::GhosttyTerminal,
    kind: sys::GhosttyTerminalData::Type,
) -> Option<T> {
    let mut v = T::default();
    let r = unsafe { sys::ghostty_terminal_get(term, kind as _, &mut v as *mut T as *mut _) };
    (r == OK).then_some(v)
}

pub(crate) fn state_get<T: Default>(
    state: sys::GhosttyRenderState,
    kind: sys::GhosttyRenderStateData::Type,
) -> Option<T> {
    let mut v = T::default();
    let r = unsafe { sys::ghostty_render_state_get(state, kind as _, &mut v as *mut T as *mut _) };
    (r == OK).then_some(v)
}
