//! Persistent app state: restorable windows, the recently closed window history, tab and
//! group metadata, split trees, preferences, and the untrusted URL policy.
//!
//! JSON layouts follow Swift's synthesized `Codable` output so files written by the
//! macOS app stay readable.

mod atomic;
mod closed_window;
mod closed_window_store;
mod geometry;
mod groups;
mod prefs;
mod restorable;
pub mod split_tree;
pub(crate) mod swift;
mod tab;
mod untrusted_url;
mod url_path;

pub use closed_window::{
    ClosedTab, ClosedWindowRecoveryContext, ClosedWindowSnapshot, RecoveryError, MAXIMUM_BYTES,
};
pub use closed_window_store::{
    CloseTicket, ClosedWindowStore, StoreError, FILE_NAME as CLOSED_WINDOW_FILE_NAME, MAXIMUM_WINDOWS,
};
pub use geometry::{Rect, Size};
pub use groups::{
    group_headers, landing, rows, selected_row_id, GroupHeader, GroupInfo, GroupLanding,
    GroupSnapshot, Row, RowId, TabGroupStore, TabItem, DEFAULT_GROUP_NAME,
};
pub use prefs::{
    clamp_sidebar_width, data_dir, default_worktree_root, Preferences, DEFAULT_TAB_SIDEBAR_WIDTH,
    FILE_NAME as PREFERENCES_FILE_NAME, TAB_SIDEBAR_WIDTH_RANGE,
};
pub use restorable::{
    is_supported_version, FullscreenMode, RestorableState, SurfaceState, MINIMUM_VERSION, VERSION,
};
pub use split_tree::SplitTree;
pub use tab::{
    closed_window_title, normalize_title_override, tab_item_title, tab_title, title_with_bell,
    window_title, HorizontalNewTabButtonPlacement, TerminalTabColor, CLOSED_WINDOW_TITLE_LIMIT,
    EMPTY_TAB_TITLE,
};
pub use untrusted_url::{
    contains_unsafe_characters, is_unsafe_character, Decision, DenialReason, UntrustedUrl,
};
pub use url_path::path_without_trailing_slash;
