//! What the terminal shows and does when its child process exits, following Ghostty's
//! `Surface.childExited`: a note in the terminal, a launch failure report for commands that
//! die immediately, and closing right away unless `wait-after-command` keeps the terminal
//! open until a key is pressed.

use std::time::Duration;

use terminal::ExitStatus;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Running,
    /// The exit was written into the terminal; `keep_open` terminals now wait for a key.
    Announced { keep_open: bool },
    Dismissed,
}

#[derive(Clone, Debug)]
pub struct ExitState {
    wait_after_command: bool,
    command_line: String,
    phase: Phase,
}

/// What a terminal needs to know to report its child's exit.
pub struct Exit<'a> {
    pub status: &'a ExitStatus,
    pub runtime: Duration,
    /// Exits faster than this with a failure status count as a launch failure.
    pub abnormal_below: Duration,
    pub cols: u16,
    /// The screen had text on it, so the report is set off by a rule.
    pub screen_has_text: bool,
}

impl ExitState {
    pub fn new(wait_after_command: bool, command_line: String) -> ExitState {
        ExitState { wait_after_command, command_line, phase: Phase::Running }
    }

    pub fn is_announced(&self) -> bool {
        self.phase != Phase::Running
    }

    /// Whether the terminal stays open showing the exit note, waiting for a key.
    pub fn is_waiting(&self) -> bool {
        matches!(self.phase, Phase::Announced { keep_open: true })
    }

    /// The terminal can be closed: it did not ask to be kept or its key was pressed.
    pub fn should_close(&self) -> bool {
        matches!(self.phase, Phase::Announced { keep_open: false } | Phase::Dismissed)
    }

    pub fn dismiss(&mut self) {
        if self.is_waiting() {
            self.phase = Phase::Dismissed;
        }
    }

    /// Records the exit and returns the output to print into the terminal. `None` when it was
    /// already announced.
    pub fn announce(&mut self, exit: &Exit) -> Option<Vec<u8>> {
        if self.phase != Phase::Running {
            return None;
        }
        let abnormal = exit.runtime <= exit.abnormal_below && !exit.status.success();
        self.phase = Phase::Announced { keep_open: abnormal || self.wait_after_command };
        Some(if abnormal { self.launch_failure(exit) } else { b"\r\nProcess exited. Press any key to close the terminal.\x1b[?25l".to_vec() })
    }

    fn launch_failure(&self, exit: &Exit) -> Vec<u8> {
        let mut out = String::from("\r\x1b[0m");
        if exit.screen_has_text {
            out.push('\n');
            out.extend(std::iter::repeat_n('\u{2501}', usize::from(exit.cols)));
            out.push_str("\r\n\n");
        }
        out.push_str("\x1b[91m\x1b[1mAngelly failed to launch the requested command:\x1b[0m\r\n\r\n");
        out.push_str(&self.command_line);
        out.push_str("\x1b[0m\r\n\r\nRuntime: \x1b[31m");
        out.push_str(&format!("{} ms\x1b[0m", exit.runtime.as_millis()));
        out.push_str(&format!("\r\nExit Code: \x1b[31m{}\x1b[0m", exit.status.code));
        out.push_str("\r\n\r\nPress any key to close the window.\x1b[?25l");
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u32) -> ExitStatus {
        ExitStatus { code, signal: None }
    }

    fn exit<'a>(status: &'a ExitStatus, millis: u64) -> Exit<'a> {
        Exit {
            status,
            runtime: Duration::from_millis(millis),
            abnormal_below: Duration::from_millis(250),
            cols: 10,
            screen_has_text: false,
        }
    }

    #[test]
    fn normal_exit_closes_immediately_unless_waiting() {
        let ok = status(0);
        let mut state = ExitState::new(false, "zsh".into());
        assert!(!state.should_close());
        let out = state.announce(&exit(&ok, 5_000)).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("Process exited. Press any key"));
        assert!(state.should_close() && !state.is_waiting());
        assert_eq!(state.announce(&exit(&ok, 5_000)), None);

        let mut waiting = ExitState::new(true, "zsh".into());
        waiting.announce(&exit(&ok, 5_000));
        assert!(waiting.is_waiting() && !waiting.should_close());
        waiting.dismiss();
        assert!(waiting.should_close() && !waiting.is_waiting());
    }

    #[test]
    fn quick_failures_stay_open_with_a_report() {
        let failed = status(127);
        let mut state = ExitState::new(false, "nosuchcmd --flag".into());
        let out = String::from_utf8(state.announce(&exit(&failed, 40)).unwrap()).unwrap();
        assert!(out.contains("failed to launch the requested command"));
        assert!(out.contains("nosuchcmd --flag"));
        assert!(out.contains("40 ms"));
        assert!(out.contains("Exit Code: \x1b[31m127"));
        assert!(!out.contains('\u{2501}'));
        assert!(state.is_waiting());

        let mut busy = ExitState::new(false, "x".into());
        let mut report = exit(&failed, 40);
        report.screen_has_text = true;
        let out = String::from_utf8(busy.announce(&report).unwrap()).unwrap();
        assert!(out.contains(&"\u{2501}".repeat(10)));

        let mut slow = ExitState::new(false, "x".into());
        slow.announce(&exit(&failed, 900));
        assert!(slow.should_close());
        let mut fast_ok = ExitState::new(false, "x".into());
        fast_ok.announce(&exit(&status(0), 10));
        assert!(fast_ok.should_close());
    }
}
