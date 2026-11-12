//! Platform pieces of the quick terminal: its screen layout and, on macOS, the native
//! panel. The window and terminal themselves belong to the application.

pub mod geometry;
#[cfg(target_os = "macos")]
pub mod panel;
