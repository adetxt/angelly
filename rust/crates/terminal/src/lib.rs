//! Terminal engine for Angelly: libghostty-vt wrapped in a safe API, plus a
//! PTY-backed [`Session`] that keeps a [`Term`] fed from a child shell.

mod command;
mod event;
mod ffi;
mod frame;
mod images;
mod input;
mod key;
mod launch;
mod process;
mod search;
mod selection;
mod session;
pub mod shell_integration;
mod term;
mod types;

pub use command::Command;
pub use event::*;
pub use frame::{Cell, Cursor, Dirty, Frame, Row};
pub use images::{Image, ImageLayer, ImagePlacement};
pub use input::{
    encode_focus, encode_paste, paste_is_safe, MouseAction, MouseButton, MouseEvent, PasteError,
    Wheel,
};
pub use key::{Key, KeyAction, KeyEvent};
pub use process::ProcessInfo;
pub use search::{SearchMatch, SearchStatus};
pub use selection::{
    Autoscroll, Pointer, ScreenPart, PressOptions, SelectionAdjust, TextFormat, DEFAULT_WORD_BOUNDARIES, REPEAT_CLICK_INTERVAL,
};
pub use session::{ExitStatus, Session, SessionConfig};
pub use term::{clipboard_read_reply, Mode, Padding, Scrollbar, Size, Term};
pub use types::*;
