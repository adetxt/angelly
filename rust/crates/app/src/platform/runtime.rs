//! Access to the GPUI app from native callbacks that arrive on the main thread
//! outside any GPUI update: Apple events, Services, Shortcuts and the global event tap.

use std::cell::RefCell;

use gpui_kit::{App, AsyncApp};

thread_local! {
    static APP: RefCell<Option<AsyncApp>> = const { RefCell::new(None) };
}

pub fn install(cx: &App) {
    APP.with(|app| *app.borrow_mut() = Some(cx.to_async()));
}

/// Runs `f` with the app. Only valid on the main thread and never from inside another
/// GPUI update; returns `None` when called from any other thread or before [`install`].
pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    let app = APP.with(|app| app.borrow().clone())?;
    Some(app.update(f))
}
