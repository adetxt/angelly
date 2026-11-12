use std::fs;
use std::os::unix::fs::PermissionsExt;

use tempfile::TempDir;

use super::*;
use crate::persist::closed_window::tests::{padded, snapshot};

fn fixture() -> (TempDir, ClosedWindowStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = ClosedWindowStore::new(directory.path().join("history"));
    (directory, store)
}

fn enabled() -> (TempDir, ClosedWindowStore) {
    let (directory, mut store) = fixture();
    store.set_persistence_enabled(true).unwrap();
    (directory, store)
}

fn ids(store: &ClosedWindowStore) -> Vec<Uuid> {
    store.entries().iter().map(|s| s.id).collect()
}

fn replace_file_with_directory(store: &ClosedWindowStore) {
    fs::remove_file(store.file_path()).unwrap();
    fs::create_dir(store.file_path()).unwrap();
}

fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[test]
fn closing_a_new_window_does_not_replace_an_older_one() {
    let (directory, mut store) = enabled();
    let first = snapshot();
    let first_ticket = store.append(first.clone()).unwrap();
    let second = snapshot();
    store.append(second.clone()).unwrap();

    let mut reloaded = ClosedWindowStore::new(directory.path().join("history"));
    reloaded.set_persistence_enabled(true).unwrap();
    assert_eq!(ids(&reloaded), vec![second.id, first.id]);

    store.consume(first.id).unwrap();
    assert!(first_ticket.is_restored());
    assert_eq!(ids(&store), vec![second.id]);
}

#[test]
fn appending_the_same_window_again_moves_it_to_the_front() {
    let (_directory, mut store) = enabled();
    let first = snapshot();
    let second = snapshot();
    store.append(first.clone()).unwrap();
    store.append(second.clone()).unwrap();
    store.append(first.clone()).unwrap();
    assert_eq!(ids(&store), vec![first.id, second.id]);
}

#[test]
fn pruning_does_not_invalidate_a_live_undo_ticket() {
    let (_directory, mut store) = enabled();
    let first = snapshot();
    let ticket = store.append(first.clone()).unwrap();
    for _ in 0..20 {
        let mut next = first.clone();
        next.id = Uuid::new_v4();
        store.append(next).unwrap();
    }
    assert_eq!(store.entries().len(), 20);
    assert!(!ids(&store).contains(&first.id));
    assert!(!ticket.is_restored());
}

#[test]
fn disabling_persistence_clears_the_history() {
    let (_directory, mut store) = enabled();
    store.append(snapshot()).unwrap();
    assert!(store.file_path().exists());
    store.set_persistence_enabled(false).unwrap();
    assert!(store.entries().is_empty());
    assert!(!store.is_persistence_enabled());
    assert!(!store.file_path().exists());
    assert_eq!(store.append(snapshot()).unwrap_err(), RecoveryError::Disabled.into());
}

#[test]
fn a_corrupt_history_is_not_overwritten_by_closing_another_window() {
    let (_directory, mut store) = fixture();
    fs::create_dir_all(store.file_path().parent().unwrap()).unwrap();
    fs::write(store.file_path(), b"invalid history").unwrap();

    assert!(store.set_persistence_enabled(true).is_err());
    assert!(store.load_failure().is_some());
    assert!(store.has_history());
    assert!(store.append(snapshot()).is_err());
    assert_eq!(fs::read(store.file_path()).unwrap(), b"invalid history");

    store.clear().unwrap();
    store.append(snapshot()).unwrap();
    assert_eq!(store.entries().len(), 1);
}

#[test]
fn a_failed_write_preserves_the_previous_history() {
    let (_directory, mut store) = enabled();
    let snapshot = snapshot();
    store.append(snapshot.clone()).unwrap();
    replace_file_with_directory(&store);

    let mut next = snapshot.clone();
    next.id = Uuid::new_v4();
    assert!(store.append(next).is_err());
    assert_eq!(ids(&store), vec![snapshot.id]);
}

#[test]
fn the_byte_limit_evicts_old_entries_and_rejects_an_oversized_window() {
    let (_directory, mut store) = enabled();
    let seven_mebibytes = 7 * 1024 * 1024;
    let large = padded(seven_mebibytes);
    store.append(large.clone()).unwrap();
    store.append(padded(seven_mebibytes)).unwrap();
    assert_eq!(store.entries().len(), 1);
    assert_ne!(ids(&store)[0], large.id);

    let previous = ids(&store);
    assert_eq!(
        store.append(padded(8 * 1024 * 1024)).unwrap_err(),
        RecoveryError::Oversized.into()
    );
    assert_eq!(ids(&store), previous);
    assert!(fs::metadata(store.file_path()).unwrap().len() <= MAXIMUM_BYTES as u64);
}

#[test]
fn a_failed_consumption_hides_the_entry_and_can_be_retried() {
    let (directory, mut store) = enabled();
    let snapshot = snapshot();
    let ticket = store.append(snapshot.clone()).unwrap();
    replace_file_with_directory(&store);

    assert!(store.consume(snapshot.id).is_err());
    assert!(ticket.is_restored());
    assert!(store.entries().is_empty());
    assert!(store.has_history());

    fs::remove_dir(store.file_path()).unwrap();
    store.consume(snapshot.id).unwrap();
    let mut reloaded = ClosedWindowStore::new(directory.path().join("history"));
    reloaded.set_persistence_enabled(true).unwrap();
    assert!(reloaded.entries().is_empty());
}

#[test]
fn files_have_owner_only_permissions() {
    let (_directory, mut store) = enabled();
    store.append(snapshot()).unwrap();
    let file = fs::metadata(store.file_path()).unwrap().permissions().mode() & 0o777;
    let parent = fs::metadata(store.file_path().parent().unwrap()).unwrap().permissions().mode() & 0o777;
    assert_eq!(file, 0o600);
    assert_eq!(parent, 0o700);
}

#[test]
fn unsupported_history_is_rejected_without_being_touched() {
    for kind in ["schema", "selection", "tabVersion"] {
        let (_directory, mut store) = fixture();
        fs::create_dir_all(store.file_path().parent().unwrap()).unwrap();
        let mut window = serde_json::to_value(snapshot()).unwrap();
        match kind {
            "selection" => window["selectedTabIndex"] = 99.into(),
            "tabVersion" => window["tabs"][0]["stateVersion"] = 999.into(),
            _ => {}
        }
        let version = if kind == "schema" { 999 } else { 1 };
        let data = serde_json::to_vec(&serde_json::json!({"version": version, "windows": [window]})).unwrap();
        fs::write(store.file_path(), &data).unwrap();

        assert!(store.set_persistence_enabled(true).is_err(), "{kind}");
        assert!(store.append(snapshot()).is_err(), "{kind}");
        assert_eq!(fs::read(store.file_path()).unwrap(), data, "{kind}");
    }
}

#[test]
fn too_many_or_duplicate_windows_are_rejected() {
    let window = snapshot();
    let cases = [
        vec![window.clone(), window.clone()],
        (0..21).map(|_| snapshot()).collect(),
    ];
    for windows in cases {
        let (_directory, mut store) = fixture();
        fs::create_dir_all(store.file_path().parent().unwrap()).unwrap();
        fs::write(
            store.file_path(),
            serde_json::to_vec(&serde_json::json!({"version": 1, "windows": windows})).unwrap(),
        )
        .unwrap();
        assert!(store.set_persistence_enabled(true).is_err());
    }
}

#[test]
fn an_oversized_history_file_is_rejected_before_decoding() {
    let (_directory, mut store) = fixture();
    fs::create_dir_all(store.file_path().parent().unwrap()).unwrap();
    fs::write(store.file_path(), vec![0u8; MAXIMUM_BYTES + 1]).unwrap();
    assert!(store.set_persistence_enabled(true).is_err());
    assert!(store.entries().is_empty());
}

#[test]
fn a_symlinked_history_file_is_rejected() {
    let (_directory, mut store) = fixture();
    let directory = store.file_path().parent().unwrap().to_path_buf();
    fs::create_dir_all(&directory).unwrap();
    let target = directory.join("target.json");
    fs::write(&target, br#"{"version":1,"windows":[]}"#).unwrap();
    std::os::unix::fs::symlink(&target, store.file_path()).unwrap();
    assert!(store.set_persistence_enabled(true).is_err());
}

#[test]
fn a_failed_deletion_still_disables_recovery_and_can_be_retried() {
    if running_as_root() {
        return;
    }
    let (_directory, mut store) = enabled();
    store.append(snapshot()).unwrap();
    let directory = store.file_path().parent().unwrap().to_path_buf();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o500)).unwrap();

    let failed = store.set_persistence_enabled(false);
    assert!(failed.is_err());
    assert!(!store.is_persistence_enabled());
    assert!(store.entries().is_empty());
    assert!(store.append(snapshot()).is_err());

    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    store.set_persistence_enabled(false).unwrap();
    assert!(!store.file_path().exists());
}

#[test]
fn a_stale_consumption_after_clear_cannot_recreate_the_history() {
    let (_directory, mut store) = enabled();
    let snapshot = snapshot();
    store.append(snapshot.clone()).unwrap();
    store.clear().unwrap();
    store.consume(snapshot.id).unwrap();
    assert!(store.entries().is_empty());
    assert!(!store.file_path().exists());
}

#[test]
fn nothing_is_read_until_persistence_is_enabled() {
    let (_directory, mut store) = fixture();
    fs::create_dir_all(store.file_path().parent().unwrap()).unwrap();
    fs::write(store.file_path(), b"invalid history").unwrap();
    store.load().unwrap();
    assert!(store.load_failure().is_none());
    assert!(store.entries().is_empty());
}
