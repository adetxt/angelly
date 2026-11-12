//! Desktop notifications for OSC 9 and OSC 777 requests and for finished commands, through
//! the freedesktop notification service on Linux and toast notifications on Windows.

use std::thread;
use std::time::Duration;

use gpui_kit::*;

use super::APP_ID;
use crate::terminal_view::{TerminalEvent, TerminalView};

pub fn install(cx: &mut App) {
    cx.observe_new(|_: &mut TerminalView, _, cx: &mut Context<TerminalView>| {
        let view = cx.entity();
        cx.subscribe(&view, |_, _, event: &TerminalEvent, _| match event {
            TerminalEvent::Notification { title, body } => show(title, body),
            TerminalEvent::CommandFinished { exit_code, duration, notify: true } => {
                let (title, body) = finished_message(*exit_code, *duration);
                show(&title, &body);
            }
            _ => {}
        })
        .detach();
    })
    .detach();
}

fn finished_message(exit_code: Option<i32>, duration: Duration) -> (String, String) {
    let title = match exit_code {
        Some(0) | None => "Command Succeeded",
        Some(_) => "Command Failed",
    };
    let seconds = duration.as_secs();
    let took = if seconds >= 60 { format!("{}m {}s", seconds / 60, seconds % 60) } else { format!("{seconds}s") };
    let body = match exit_code {
        Some(code) if code != 0 => format!("Exited with code {code} after {took}."),
        _ => format!("Command took {took}."),
    };
    (title.to_owned(), body)
}

/// Posts a notification without blocking the UI thread on the notification service.
pub fn show(title: &str, body: &str) {
    let (title, body) = (title.to_owned(), body.to_owned());
    thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("Angelly").summary(&title).body(&body);
        #[cfg(all(unix, not(target_os = "macos")))]
        notification.icon(APP_ID);
        #[cfg(windows)]
        notification.app_id(APP_ID);
        if let Err(error) = notification.show() {
            eprintln!("angelly: desktop notification failed: {error}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_command_reports_its_exit_code() {
        let (title, body) = finished_message(Some(2), Duration::from_secs(75));
        assert_eq!(title, "Command Failed");
        assert_eq!(body, "Exited with code 2 after 1m 15s.");
    }

    #[test]
    fn a_successful_command_reports_its_duration() {
        let (title, body) = finished_message(Some(0), Duration::from_secs(9));
        assert_eq!(title, "Command Succeeded");
        assert_eq!(body, "Command took 9s.");
    }
}
