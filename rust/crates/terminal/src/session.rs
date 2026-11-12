use std::{
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, MutexGuard, PoisonError,
    },
    thread,
    time::Duration,
};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

use crate::{
    launch,
    process::{self, ProcessInfo},
    shell_integration::ShellIntegration,
    term::Term,
    types::{Colors, OptionAsAlt},
};

pub struct SessionConfig {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u32,
    pub cell_height: u32,
    /// Program and arguments. `None` runs `$SHELL -l` (falling back to
    /// `/bin/sh`); on Windows, PowerShell 7, Windows PowerShell or `%COMSPEC%`.
    pub command: Option<Vec<String>>,
    /// Working directory; defaults to the user's home directory.
    pub cwd: Option<PathBuf>,
    /// Measure grapheme clusters by Unicode rules (`grapheme-width-method = unicode`).
    pub grapheme_clustering: bool,
    /// Extra environment, applied after the defaults below.
    pub env: Vec<(String, String)>,
    /// The configured terminal name. `xterm-ghostty` is only used when the terminfo bundled
    /// next to `resources_dir` exists; otherwise the child gets `xterm-256color`. The name
    /// the child ends up with is also what XTGETTCAP reports.
    pub term: String,
    /// The bundled resources (`Contents/Resources/ghostty`): enables shell integration and
    /// `TERMINFO`, and is exported as `GHOSTTY_RESOURCES_DIR`.
    pub resources_dir: Option<PathBuf>,
    pub shell_integration: ShellIntegration,
    /// Exported as `TERM_PROGRAM_VERSION`.
    pub term_program_version: Option<String>,
    pub scrollback_bytes: Option<usize>,
    pub colors: Option<Colors>,
    pub option_as_alt: OptionAsAlt,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 16,
            command: None,
            cwd: None,
            grapheme_clustering: true,
            env: Vec::new(),
            term: "xterm-ghostty".into(),
            resources_dir: None,
            shell_integration: ShellIntegration::default(),
            term_program_version: None,
            scrollback_bytes: None,
            colors: None,
            option_as_alt: OptionAsAlt::False,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitStatus {
    pub code: u32,
    /// Name of the terminating signal, if the child was killed by one.
    pub signal: Option<String>,
}

impl ExitStatus {
    pub fn success(&self) -> bool {
        self.code == 0 && self.signal.is_none()
    }
}

struct Shared {
    term: Mutex<Term>,
    wake_pending: AtomicBool,
    reader_done: AtomicBool,
    exit: Mutex<Option<ExitStatus>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

impl Shared {
    /// Coalesced: the callback runs once until the UI next takes the term lock.
    fn signal(&self) {
        if !self.wake_pending.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
}

/// A shell running on a PTY with its output parsed into a shared [`Term`].
///
/// Reader and waiter threads run in the background; `wake` is invoked from
/// them whenever there is something new to draw (output, title change, exit).
/// The callback is coalesced and must not block; the usual implementation
/// nudges the UI thread, which then calls [`Session::term`].
pub struct Session {
    shared: Arc<Shared>,
    input: mpsc::Sender<Vec<u8>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pid: Option<u32>,
}

impl Session {
    pub fn spawn(
        config: SessionConfig,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> io::Result<Session> {
        let size = PtySize {
            rows: config.rows,
            cols: config.cols,
            pixel_width: (config.cols as u32 * config.cell_width).min(u16::MAX as u32) as u16,
            pixel_height: (config.rows as u32 * config.cell_height).min(u16::MAX as u32) as u16,
        };
        let pair = native_pty_system().openpty(size).map_err(to_io)?;

        let command = config.command.clone().unwrap_or_else(default_shell);
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(PathBuf::from));
        let launch = launch::build(
            &launch::Inputs {
                command: &command,
                env: &config.env,
                term: &config.term,
                resources: config.resources_dir.as_deref(),
                integration: &config.shell_integration,
                exe_dir: exe_dir.as_deref(),
                term_program_version: config.term_program_version.as_deref(),
            },
            std::env::vars().collect(),
        );
        let (program, args) = launch
            .argv
            .split_first()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty command"))?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        if let Some(cwd) = &config.cwd {
            cmd.cwd(cwd);
        }
        cmd.env_clear();
        for (key, value) in &launch.env {
            cmd.env(key, value);
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(to_io)?;
        drop(pair.slave);
        let pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader().map_err(to_io)?;
        let mut writer = pair.master.take_writer().map_err(to_io)?;

        let mut term = Term::new(config.cols, config.rows);
        term.resize(
            config.cols,
            config.rows,
            config.cell_width,
            config.cell_height,
        );
        term.set_terminal_name(&launch.term);
        term.set_option_as_alt(config.option_as_alt);
        term.set_grapheme_clustering(config.grapheme_clustering);
        if let Some(bytes) = config.scrollback_bytes {
            term.set_scrollback_limit_bytes(bytes);
        }
        if let Some(colors) = &config.colors {
            term.set_colors(colors);
        }

        let shared = Arc::new(Shared {
            term: Mutex::new(term),
            wake_pending: AtomicBool::new(false),
            reader_done: AtomicBool::new(false),
            exit: Mutex::new(None),
            wake: Box::new(wake),
        });

        // PTY writes go through a thread so a child that stops reading can
        // never block the UI or the reader (which also sends query replies).
        let (input, rx) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("pty-writer".into())
            .spawn(move || {
                for chunk in rx {
                    if writer
                        .write_all(&chunk)
                        .and_then(|_| writer.flush())
                        .is_err()
                    {
                        break;
                    }
                }
            })?;

        let s = shared.clone();
        let replies = input.clone();
        thread::Builder::new()
            .name("pty-reader".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    let n = match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => n,
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    };
                    let reply = {
                        let mut term = s.term.lock().unwrap_or_else(PoisonError::into_inner);
                        term.write(&buf[..n]);
                        term.take_pty_output()
                    };
                    if !reply.is_empty() {
                        let _ = replies.send(reply);
                    }
                    s.signal();
                }
                s.reader_done.store(true, Ordering::Release);
                s.signal();
            })?;

        let s = shared.clone();
        thread::Builder::new()
            .name("pty-waiter".into())
            .spawn(move || {
                let status = child.wait();
                // Give the reader a moment to drain output written just before exit.
                for _ in 0..60 {
                    if s.reader_done.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                let status = match status {
                    Ok(st) => ExitStatus {
                        code: st.exit_code(),
                        signal: st.signal().map(str::to_owned),
                    },
                    Err(_) => ExitStatus {
                        code: 1,
                        signal: None,
                    },
                };
                *s.exit.lock().unwrap_or_else(PoisonError::into_inner) = Some(status);
                s.signal();
            })?;

        Ok(Session {
            shared,
            input,
            master: Mutex::new(pair.master),
            killer: Mutex::new(killer),
            pid,
        })
    }

    /// Locks the terminal. Taking the lock acknowledges the wake signal, so
    /// output arriving afterwards triggers the callback again.
    pub fn term(&self) -> MutexGuard<'_, Term> {
        let guard = self
            .shared
            .term
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.shared.wake_pending.store(false, Ordering::Release);
        guard
    }

    /// Queues bytes for the child's stdin (keystrokes, pastes, reports).
    pub fn write(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.input.send(bytes.to_vec());
        }
    }

    /// Resizes the terminal grid and the PTY (`TIOCSWINSZ`).
    pub fn resize(&self, cols: u16, rows: u16, cell_width: u32, cell_height: u32) {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let reply = {
            let mut term = self.term();
            term.resize(cols, rows, cell_width, cell_height);
            term.take_pty_output()
        };
        let size = PtySize {
            rows,
            cols,
            pixel_width: (cols as u32 * cell_width).min(u16::MAX as u32) as u16,
            pixel_height: (rows as u32 * cell_height).min(u16::MAX as u32) as u16,
        };
        let _ = self
            .master
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .resize(size);
        self.write(&reply);
    }

    /// `None` while the child is running.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.shared
            .exit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Whether the foreground program waits for a line with echo off, as `sudo` and `ssh` do
    /// for a password. Ghostty polls the pty's terminal modes the same way.
    pub fn password_input(&self) -> bool {
        #[cfg(unix)]
        {
            let master = self.master.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(fd) = master.as_raw_fd() else { return false };
            let mut modes = std::mem::MaybeUninit::<libc::termios>::zeroed();
            let read = unsafe { libc::tcgetattr(fd, modes.as_mut_ptr()) } == 0;
            read && is_password_mode(unsafe { modes.assume_init() }.c_lflag)
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    /// Asks the child to hang up (SIGHUP).
    pub fn kill(&self) {
        let _ = self
            .killer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .kill();
    }

    /// The process currently in the PTY's foreground group, with its name and
    /// working directory.
    pub fn foreground_process(&self) -> Option<ProcessInfo> {
        process::lookup(self.foreground_pid()?)
    }

    #[cfg(unix)]
    fn foreground_pid(&self) -> Option<u32> {
        let pgid = self
            .master
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .process_group_leader()?;
        Some(pgid as u32)
    }

    #[cfg(windows)]
    fn foreground_pid(&self) -> Option<u32> {
        process::foreground_descendant(self.pid?)
    }

    /// Where a new tab should start: the directory the shell last reported
    /// via OSC 7, else the shell process's own working directory.
    pub fn cwd(&self) -> Option<PathBuf> {
        if let Some(path) = self.term().pwd_path() {
            return Some(PathBuf::from(path));
        }
        process::lookup(self.pid?)?.cwd
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.exit_status().is_none() {
            self.kill();
        }
    }
}

#[cfg(unix)]
fn default_shell() -> Vec<String> {
    vec![
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
        "-l".into(),
    ]
}

#[cfg(windows)]
fn default_shell() -> Vec<String> {
    let on_path = |name: &str| {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    };
    let shell = ["pwsh.exe", "powershell.exe"]
        .iter()
        .find_map(|name| on_path(name))
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| std::env::var("COMSPEC").ok())
        .unwrap_or_else(|| "cmd.exe".into());
    vec![shell]
}

fn to_io(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

/// Echo off with line editing on: a program reading a secret.
#[cfg(unix)]
fn is_password_mode(local_flags: libc::tcflag_t) -> bool {
    local_flags & libc::ECHO == 0 && local_flags & libc::ICANON != 0
}

#[cfg(all(test, unix))]
mod password_tests {
    use super::*;

    #[test]
    fn a_secret_is_read_with_echo_off_in_line_mode() {
        assert!(is_password_mode(libc::ICANON));
        assert!(!is_password_mode(libc::ICANON | libc::ECHO));
        assert!(!is_password_mode(0), "raw mode is a full-screen program, not a prompt");
        assert!(!is_password_mode(libc::ECHO));
    }
}
