//! Persistent history of recently closed windows.
//!
//! The history is one JSON file. A file that cannot be read back is never overwritten:
//! the failure is remembered and every write is refused until the history is cleared.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::closed_window::{ClosedWindowSnapshot, RecoveryError, MAXIMUM_BYTES};
use super::atomic::write_private_atomic;

pub const FILE_NAME: &str = "recently-closed-windows.json";
pub const MAXIMUM_WINDOWS: usize = 20;
const HISTORY_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    Recovery(RecoveryError),
    Io(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recovery(error) => error.fmt(f),
            Self::Io(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<RecoveryError> for StoreError {
    fn from(error: RecoveryError) -> Self {
        Self::Recovery(error)
    }
}

impl From<io::Error> for StoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Handed out when a window is appended so an undo can tell whether the window was
/// reopened in the meantime. Clones share the same state.
#[derive(Debug, Clone)]
pub struct CloseTicket {
    id: Uuid,
    restored: Arc<AtomicBool>,
}

impl CloseTicket {
    fn new(id: Uuid) -> Self {
        Self { id, restored: Arc::new(AtomicBool::new(false)) }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn is_restored(&self) -> bool {
        self.restored.load(Ordering::SeqCst)
    }
}

#[derive(Serialize, Deserialize)]
struct History {
    version: i64,
    windows: Vec<ClosedWindowSnapshot>,
}

#[derive(Debug)]
pub struct ClosedWindowStore {
    file: PathBuf,
    persistence_enabled: bool,
    load_failure: Option<StoreError>,
    snapshots: Vec<ClosedWindowSnapshot>,
    tickets: HashMap<Uuid, CloseTicket>,
}

impl ClosedWindowStore {
    /// A store keeping its history file inside `directory`. Nothing is read or written
    /// until persistence is enabled.
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            file: directory.as_ref().join(FILE_NAME),
            persistence_enabled: false,
            load_failure: None,
            snapshots: Vec::new(),
            tickets: HashMap::new(),
        }
    }

    pub fn file_path(&self) -> &Path {
        &self.file
    }

    pub fn is_persistence_enabled(&self) -> bool {
        self.persistence_enabled
    }

    pub fn load_failure(&self) -> Option<&StoreError> {
        self.load_failure.as_ref()
    }

    pub fn has_history(&self) -> bool {
        !self.snapshots.is_empty() || self.load_failure.is_some()
    }

    /// Reopenable windows, newest first. Windows already being reopened are left out.
    pub fn entries(&self) -> Vec<&ClosedWindowSnapshot> {
        if !self.persistence_enabled || self.load_failure.is_some() {
            return Vec::new();
        }
        self.snapshots
            .iter()
            .filter(|snapshot| !self.tickets.get(&snapshot.id).is_some_and(CloseTicket::is_restored))
            .collect()
    }

    /// Enabling loads the saved history; disabling deletes it. Disabling takes effect
    /// even when the deletion fails, and the deletion can be retried.
    pub fn set_persistence_enabled(&mut self, enabled: bool) -> Result<(), StoreError> {
        if enabled {
            if self.persistence_enabled {
                return Ok(());
            }
            self.persistence_enabled = true;
            self.load()
        } else {
            self.persistence_enabled = false;
            self.clear()
        }
    }

    pub fn load(&mut self) -> Result<(), StoreError> {
        if !self.persistence_enabled {
            return Ok(());
        }
        match self.read_history() {
            Ok(windows) => {
                self.snapshots = windows;
                self.load_failure = None;
                Ok(())
            }
            Err(error) => {
                self.load_failure = Some(error.clone());
                Err(error)
            }
        }
    }

    fn read_history(&self) -> Result<Vec<ClosedWindowSnapshot>, StoreError> {
        let metadata = match fs::symlink_metadata(&self.file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_file() || metadata.len() > MAXIMUM_BYTES as u64 {
            return Err(RecoveryError::InvalidHistory.into());
        }
        let data = fs::read(&self.file)?;
        if data.len() > MAXIMUM_BYTES {
            return Err(RecoveryError::InvalidHistory.into());
        }
        let history: History =
            serde_json::from_slice(&data).map_err(|_| RecoveryError::InvalidHistory)?;
        let mut ids: Vec<Uuid> = history.windows.iter().map(|w| w.id).collect();
        ids.sort();
        ids.dedup();
        if history.version != HISTORY_VERSION
            || history.windows.len() > MAXIMUM_WINDOWS
            || ids.len() != history.windows.len()
        {
            return Err(RecoveryError::InvalidHistory.into());
        }
        for snapshot in &history.windows {
            snapshot.validate()?;
        }
        Ok(history.windows)
    }

    /// Records a closed window as the newest entry, dropping the oldest entries to stay
    /// within the window count and byte limits.
    pub fn append(&mut self, snapshot: ClosedWindowSnapshot) -> Result<CloseTicket, StoreError> {
        if !self.persistence_enabled {
            return Err(RecoveryError::Disabled.into());
        }
        if let Some(failure) = &self.load_failure {
            return Err(failure.clone());
        }
        snapshot.validate()?;
        if encode(std::slice::from_ref(&snapshot))?.len() > MAXIMUM_BYTES {
            return Err(RecoveryError::Oversized.into());
        }

        let id = snapshot.id;
        let mut next = vec![snapshot];
        next.extend(self.entries().into_iter().filter(|s| s.id != id).cloned());
        next.truncate(MAXIMUM_WINDOWS);
        while next.len() > 1 && encode(&next)?.len() > MAXIMUM_BYTES {
            next.pop();
        }

        self.commit(next)?;
        let ticket = CloseTicket::new(id);
        let kept: Vec<Uuid> = self.snapshots.iter().map(|s| s.id).collect();
        self.tickets.retain(|ticket_id, _| kept.contains(ticket_id));
        self.tickets.insert(id, ticket.clone());
        Ok(ticket)
    }

    /// The ticket for `id`, creating one if the window has none yet.
    pub fn ticket(&mut self, id: Uuid) -> CloseTicket {
        self.tickets.entry(id).or_insert_with(|| CloseTicket::new(id)).clone()
    }

    /// Marks a window as reopened and removes it from the history. The window stays hidden
    /// from [`entries`](Self::entries) even if the write fails, and the removal can be retried.
    pub fn consume(&mut self, id: Uuid) -> Result<(), StoreError> {
        self.ticket(id).restored.store(true, Ordering::SeqCst);
        if !self.persistence_enabled || !self.snapshots.iter().any(|s| s.id == id) {
            return Ok(());
        }
        let remaining = self.snapshots.iter().filter(|s| s.id != id).cloned().collect();
        self.commit(remaining)?;
        self.tickets.remove(&id);
        Ok(())
    }

    /// Forgets the history and deletes its file.
    pub fn clear(&mut self) -> Result<(), StoreError> {
        self.snapshots.clear();
        self.tickets.clear();
        self.load_failure = None;
        let removal = match fs::symlink_metadata(&self.file) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(&self.file),
            Ok(_) => fs::remove_file(&self.file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
        removal.map_err(|error| {
            let error = StoreError::from(error);
            self.load_failure = Some(error.clone());
            error
        })
    }

    fn commit(&mut self, windows: Vec<ClosedWindowSnapshot>) -> Result<(), StoreError> {
        write_private_atomic(&self.file, &encode(&windows)?)?;
        self.snapshots = windows;
        Ok(())
    }
}

fn encode(windows: &[ClosedWindowSnapshot]) -> Result<Vec<u8>, StoreError> {
    #[derive(Serialize)]
    struct HistoryRef<'a> {
        version: i64,
        windows: &'a [ClosedWindowSnapshot],
    }
    serde_json::to_vec(&HistoryRef { version: HISTORY_VERSION, windows })
        .map_err(|error| StoreError::Io(error.to_string()))
}

#[cfg(test)]
mod tests;
