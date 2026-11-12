//! Update checking against GitHub releases and the state shown by the update pill.
//!
//! Sparkle did more than this crate replaces. Still needed from the platform layer:
//! unpacking the DMG, swapping the app bundle after the app quits, relaunching, and
//! the Sparkle-specific EdDSA signature check (this crate verifies the SHA-256 that
//! each release publishes beside its package instead). Sparkle also stored the
//! last-check time and the automatic-check opt-in in user defaults and showed a
//! permission prompt on the second launch; [`automatic_check_due`] and
//! [`UpdateController::request_permission`] cover the logic, persistence is the caller's.

pub mod controller;
pub mod download;
pub mod release;
pub mod state;
pub mod version;

pub use controller::{automatic_check_due, CheckStart, UpdateController, AUTOMATIC_CHECK_INTERVAL};
pub use download::{download_release, parse_checksum, sha256_file, Downloader, UreqDownloader};
pub use release::{CheckOutcome, ReleaseAsset, ReleaseInfo, UpdateChecker, UpdateError, DEFAULT_REPOSITORY};
pub use state::{ReleaseNotes, Tone, UpdateIcon, UpdateState};
pub use version::{compare_versions, is_newer};
