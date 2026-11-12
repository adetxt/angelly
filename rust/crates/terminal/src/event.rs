/// Notifications raised by the terminal while processing output. Drain them
/// with [`Term::take_events`](crate::Term::take_events).
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Bell,
    /// Window title changed (OSC 0/2). Carries the latest title.
    Title(String),
    /// Working directory changed (OSC 7). Carries the raw reported value,
    /// normally a `file://host/path` URL; see [`Term::pwd_path`](crate::Term::pwd_path).
    Pwd(String),
    /// OSC 9;4 progress report.
    Progress(Progress),
    /// OSC 9 / OSC 777 desktop notification.
    Notification {
        title: String,
        body: String,
    },
    /// OSC 52 (or kitty/iTerm2 clipboard) write. `text` is `None` when the
    /// program asked to clear the clipboard.
    ClipboardWrite {
        location: ClipboardLocation,
        text: Option<String>,
    },
    /// OSC 52 read request (`?` payload). Answer it with
    /// [`clipboard_read_reply`](crate::clipboard_read_reply) or not at all.
    ClipboardRead {
        location: ClipboardLocation,
    },
    /// OSC 7501 program status report.
    ProgramStatus(ProgramStatus),
    /// OSC 133 shell integration marker.
    SemanticPrompt {
        kind: PromptKind,
        exit_code: Option<i32>,
    },
    /// The program asked to resize the window (CSI 8 ; rows ; cols t).
    ResizeRequest {
        cols: u16,
        rows: u16,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgressState {
    Remove,
    Set,
    Error,
    Indeterminate,
    Pause,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Progress {
    pub state: ProgressState,
    pub percent: Option<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardLocation {
    Standard,
    Selection,
    Primary,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    Clear,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgramKind {
    None,
    Permission,
    Question,
    Auth,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProgramStatus {
    pub state: ProgramState,
    pub kind: ProgramKind,
    pub progress: Option<u8>,
    pub id: String,
    pub app: String,
    pub title: String,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromptKind {
    PromptStart,
    InputStart,
    OutputStart,
    CommandEnd,
}
