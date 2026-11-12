//! A closed window saved so it can be reopened: its tabs' restorable state plus window geometry.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::geometry::Rect;
use super::restorable::{self, RestorableState, SurfaceState};
use super::split_tree::Node;
use super::swift;
use super::tab::CLOSED_WINDOW_TITLE_LIMIT;

/// Upper bound for a history file and for the saved state of one window, in bytes.
pub const MAXIMUM_BYTES: usize = 10 * 1024 * 1024;
const MAXIMUM_TABS: usize = 256;
const MAXIMUM_TREE_DEPTH: usize = 64;
const MAXIMUM_SURFACES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryError {
    Unavailable,
    InvalidHistory,
    Oversized,
    Disabled,
}

impl fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unavailable => "This window contains a terminal that cannot be saved for recovery.",
            Self::InvalidHistory => "The saved window history is damaged or uses an unsupported format.",
            Self::Oversized => "This window exceeds the recovery history size limit.",
            Self::Disabled => "Window recovery is disabled.",
        })
    }
}

impl std::error::Error for RecoveryError {}

/// One tab's saved state: a JSON-encoded [`RestorableState`] and the version it was written with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedTab {
    pub state_version: i64,
    #[serde(with = "swift::data")]
    pub state_data: Vec<u8>,
}

impl ClosedTab {
    /// Encodes `state` at the current version.
    pub fn encode(state: &RestorableState<SurfaceState>) -> serde_json::Result<Self> {
        Ok(Self { state_version: restorable::VERSION, state_data: serde_json::to_vec(state)? })
    }

    /// Decodes the saved state without touching any terminals.
    pub fn decode(&self) -> Result<RestorableState<SurfaceState>, RecoveryError> {
        serde_json::from_slice(&self.state_data).map_err(|_| RecoveryError::InvalidHistory)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClosedWindowSnapshot {
    #[serde(with = "swift::uuid")]
    pub id: Uuid,
    #[serde(with = "swift::date")]
    pub closed_at: SystemTime,
    pub title: String,
    pub frame: Rect,
    pub selected_tab_index: i64,
    pub tabs: Vec<ClosedTab>,
}

impl ClosedWindowSnapshot {
    /// Snapshot of a window with a fresh id. `title` is cut to the title limit.
    pub fn new(
        title: &str,
        frame: Rect,
        selected_tab_index: usize,
        tabs: Vec<ClosedTab>,
        closed_at: SystemTime,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            closed_at,
            title: title.chars().take(CLOSED_WINDOW_TITLE_LIMIT).collect(),
            frame,
            selected_tab_index: selected_tab_index as i64,
            tabs,
        }
    }

    /// Checks the snapshot is well formed and bounded, decoding every tab's state.
    pub fn validate(&self) -> Result<(), RecoveryError> {
        let invalid = Err(RecoveryError::InvalidHistory);
        let selected_in_range = usize::try_from(self.selected_tab_index)
            .is_ok_and(|index| index < self.tabs.len());
        let total_bytes: usize = self.tabs.iter().map(|tab| tab.state_data.len()).sum();
        if self.tabs.is_empty()
            || self.tabs.len() > MAXIMUM_TABS
            || !selected_in_range
            || self.title.chars().count() > CLOSED_WINDOW_TITLE_LIMIT
            || !self.frame.is_finite()
            || self.frame.width <= 0.0
            || self.frame.height <= 0.0
            || total_bytes > MAXIMUM_BYTES
        {
            return invalid;
        }

        let mut ids = HashSet::new();
        for tab in &self.tabs {
            if !restorable::is_supported_version(tab.state_version) {
                return invalid;
            }
            let state = tab.decode()?;
            let Some(root) = state.surface_tree.root() else {
                return invalid;
            };
            validate_node(root, 0, &mut ids)?;
            if let Some(focused) = &state.focused_surface {
                if !state.surface_tree.views().iter().any(|view| &view.id_string() == focused) {
                    return invalid;
                }
            }
        }
        Ok(())
    }
}

fn validate_node(
    node: &Node<SurfaceState>,
    depth: usize,
    ids: &mut HashSet<Uuid>,
) -> Result<(), RecoveryError> {
    if depth >= MAXIMUM_TREE_DEPTH || ids.len() >= MAXIMUM_SURFACES {
        return Err(RecoveryError::InvalidHistory);
    }
    match node {
        Node::Leaf(view) => {
            if !ids.insert(view.uuid) {
                return Err(RecoveryError::InvalidHistory);
            }
        }
        Node::Split(split) => {
            if !(split.ratio.is_finite() && split.ratio > 0.0 && split.ratio < 1.0) {
                return Err(RecoveryError::InvalidHistory);
            }
            validate_node(&split.left, depth + 1, ids)?;
            validate_node(&split.right, depth + 1, ids)?;
        }
    }
    Ok(())
}

/// Gives a recovered window fresh identities. Only explicit recovery remaps ids; launch
/// restoration keeps the saved ones.
#[derive(Debug, Default)]
pub struct ClosedWindowRecoveryContext {
    ids: HashMap<Uuid, Uuid>,
    /// Set when a saved working directory no longer exists and the default was used.
    pub used_directory_fallback: bool,
}

impl ClosedWindowRecoveryContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// The new id standing in for a saved id; the same saved id always maps to the same new id.
    pub fn replacement(&mut self, id: Uuid) -> Uuid {
        *self.ids.entry(id).or_insert_with(Uuid::new_v4)
    }

    /// Remaps every id in `state` (surfaces, the saved focus and the sidebar group) and
    /// clears working directories for which `is_directory` is false.
    pub fn recover(
        &mut self,
        state: RestorableState<SurfaceState>,
        is_directory: impl Fn(&str) -> bool,
    ) -> RestorableState<SurfaceState> {
        let surface_tree = state.surface_tree.map(|surface| {
            let mut surface = surface.clone();
            surface.uuid = self.replacement(surface.uuid);
            if let Some(path) = &surface.pwd {
                if !is_directory(path) {
                    surface.pwd = None;
                    self.used_directory_fallback = true;
                }
            }
            surface
        });
        let focused_surface = state.focused_surface.map(|saved| match Uuid::parse_str(&saved) {
            Ok(id) => swift::uuid::string(&self.replacement(id)),
            Err(_) => saved,
        });
        let sidebar_group = state.sidebar_group.map(|mut group| {
            group.id = self.replacement(group.id);
            group
        });
        RestorableState {
            focused_surface,
            surface_tree,
            sidebar_group,
            ..state
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
