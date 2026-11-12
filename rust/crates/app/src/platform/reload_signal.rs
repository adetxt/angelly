//! `SIGUSR2` reloads the configuration, as in Ghostty. The handler only sets a flag; a task
//! on the main thread notices it and runs the reload.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::App;

use super::{command, Command};

const POLL: Duration = Duration::from_millis(500);

static REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    REQUESTED.store(true, Ordering::Relaxed);
}

pub fn install(cx: &mut App) {
    // SAFETY: the handler is async-signal-safe: it only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGUSR2, on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    cx.spawn(async move |cx| loop {
        cx.background_executor().timer(POLL).await;
        if REQUESTED.swap(false, Ordering::Relaxed) {
            cx.update(|cx| command::perform(Command::ReloadConfig, cx));
        }
    })
    .detach();
}
