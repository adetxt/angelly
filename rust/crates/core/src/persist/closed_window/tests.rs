use std::time::{Duration, SystemTime};

use super::*;
use crate::persist::groups::{GroupInfo, GroupSnapshot};
use crate::persist::split_tree::{NewDirection, SplitTree};
use crate::persist::tab::TerminalTabColor;

pub(crate) fn surface() -> SurfaceState {
    SurfaceState::new(Uuid::new_v4())
}

pub(crate) fn state() -> RestorableState<SurfaceState> {
    RestorableState::new(SplitTree::with_view(surface()))
}

pub(crate) fn snapshot_of(states: &[RestorableState<SurfaceState>]) -> ClosedWindowSnapshot {
    let tabs = states.iter().map(|s| ClosedTab::encode(s).unwrap()).collect();
    ClosedWindowSnapshot::new(
        "Work",
        Rect::new(100.0, 100.0, 640.0, 480.0),
        0,
        tabs,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    )
}

pub(crate) fn snapshot() -> ClosedWindowSnapshot {
    snapshot_of(&[state()])
}

/// A snapshot whose single tab carries `bytes` of extra state, for size limit tests.
pub(crate) fn padded(bytes: usize) -> ClosedWindowSnapshot {
    let mut value = serde_json::to_value(state()).unwrap();
    value["padding"] = "x".repeat(bytes).into();
    let mut snapshot = snapshot();
    snapshot.id = Uuid::new_v4();
    snapshot.tabs = vec![ClosedTab {
        state_version: restorable::VERSION,
        state_data: serde_json::to_vec(&value).unwrap(),
    }];
    snapshot
}

fn with_state_json(edit: impl FnOnce(&mut serde_json::Value)) -> ClosedWindowSnapshot {
    let mut snapshot = snapshot();
    let mut value: serde_json::Value = serde_json::from_slice(&snapshot.tabs[0].state_data).unwrap();
    edit(&mut value);
    snapshot.tabs[0].state_data = serde_json::to_vec(&value).unwrap();
    snapshot
}

#[test]
fn a_captured_snapshot_round_trips_and_validates() {
    let first = RestorableState {
        title_override: Some("Work".into()),
        tab_color: Some(TerminalTabColor::Purple),
        ..state()
    };
    let second = RestorableState { title_override: Some("Server".into()), ..state() };
    let snapshot = snapshot_of(&[first, second]);

    let json = serde_json::to_vec(&snapshot).unwrap();
    let decoded: ClosedWindowSnapshot = serde_json::from_slice(&json).unwrap();
    decoded.validate().unwrap();

    assert_eq!(decoded, snapshot);
    assert_eq!(decoded.tabs.len(), 2);
    assert_eq!(decoded.selected_tab_index, 0);
    assert_eq!(decoded.title, "Work");
    assert_eq!(decoded.tabs[0].decode().unwrap().tab_color, Some(TerminalTabColor::Purple));
    assert_eq!(decoded.tabs[1].decode().unwrap().title_override.as_deref(), Some("Server"));
}

#[test]
fn json_matches_the_swift_layout() {
    let snapshot = snapshot();
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value["id"], serde_json::json!(super::swift::uuid::string(&snapshot.id)));
    assert_eq!(value["frame"], serde_json::json!([[100.0, 100.0], [640.0, 480.0]]));
    assert_eq!(value["selectedTabIndex"], 0);
    // 1_700_000_000 s after 1970 is 721_692_800 s after the 2001 reference date.
    assert_eq!(value["closedAt"], 721_692_800.0);
    assert_eq!(value["tabs"][0]["stateVersion"], 8);
    assert!(value["tabs"][0]["stateData"].is_string());
}

#[test]
fn titles_are_truncated_on_capture() {
    let long = "x".repeat(300);
    let snapshot = ClosedWindowSnapshot::new(&long, Rect::new(0.0, 0.0, 1.0, 1.0), 0, vec![], SystemTime::UNIX_EPOCH);
    assert_eq!(snapshot.title.chars().count(), 120);
}

#[test]
fn rejects_empty_oversized_or_misindexed_tab_lists() {
    let mut snapshot = snapshot();
    snapshot.selected_tab_index = 1;
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
    snapshot.selected_tab_index = -1;
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
    snapshot.selected_tab_index = 0;
    snapshot.validate().unwrap();

    let tab = snapshot.tabs[0].clone();
    snapshot.tabs = vec![];
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
    snapshot.tabs = vec![tab; 257];
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn rejects_bad_geometry_and_titles() {
    for frame in [
        Rect::new(0.0, 0.0, 0.0, 480.0),
        Rect::new(0.0, 0.0, 640.0, -1.0),
        Rect::new(f64::NAN, 0.0, 640.0, 480.0),
        Rect::new(0.0, f64::INFINITY, 640.0, 480.0),
    ] {
        let mut snapshot = snapshot();
        snapshot.frame = frame;
        assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory), "{frame:?}");
    }
    let mut snapshot = snapshot();
    snapshot.title = "x".repeat(121);
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn rejects_unsupported_state_versions() {
    for version in [4, 9, 999] {
        let mut snapshot = snapshot();
        snapshot.tabs[0].state_version = version;
        assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory), "{version}");
    }
    for version in [5, 6, 7, 8] {
        let mut snapshot = snapshot();
        snapshot.tabs[0].state_version = version;
        snapshot.validate().unwrap();
    }
}

#[test]
fn invalid_payloads_are_rejected_before_anything_is_restored() {
    let mut snapshot = snapshot();
    snapshot.tabs[0].state_data = b"{}".to_vec();
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
    snapshot.tabs[0].state_data = b"not json".to_vec();
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn rejects_an_empty_surface_tree() {
    let snapshot = snapshot_of(&[RestorableState::new(SplitTree::default())]);
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn rejects_duplicate_surface_ids_across_tabs() {
    let shared = surface();
    let tab = |surface: &SurfaceState| RestorableState::new(SplitTree::with_view(surface.clone()));
    let snapshot = snapshot_of(&[tab(&shared), tab(&shared)]);
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn rejects_split_ratios_outside_the_open_unit_interval() {
    for ratio in [0.0, 1.0, -0.5, 1.5] {
        let a = surface();
        let tree = SplitTree::with_view(a.clone())
            .inserting(surface(), &a, NewDirection::Right)
            .unwrap();
        let snapshot = with_state_json(|value| {
            *value = serde_json::to_value(RestorableState::new(tree)).unwrap();
            value["surfaceTree"]["root"]["split"]["ratio"] = ratio.into();
        });
        assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory), "{ratio}");
    }
}

#[test]
fn rejects_trees_that_are_too_deep_or_too_large() {
    let mut deep = SplitTree::with_view(surface());
    let mut last = deep.views()[0].clone();
    for _ in 0..70 {
        let next = surface();
        deep = deep.inserting(next.clone(), &last, NewDirection::Right).unwrap();
        last = next;
    }
    assert_eq!(snapshot_of(&[RestorableState::new(deep)]).validate(), Err(RecoveryError::InvalidHistory));

    let mut shallow = SplitTree::with_view(surface());
    let mut anchor = shallow.views()[0].clone();
    for _ in 0..20 {
        let next = surface();
        shallow = shallow.inserting(next.clone(), &anchor, NewDirection::Right).unwrap();
        anchor = next;
    }
    snapshot_of(&[RestorableState::new(shallow)]).validate().unwrap();

    let wide: Vec<_> = (0..257).map(|_| state()).collect();
    assert_eq!(snapshot_of(&wide).validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn a_saved_focus_must_name_a_surface_in_the_tree() {
    let mut focused = state();
    focused.focused_surface = Some(focused.surface_tree.views()[0].id_string());
    snapshot_of(&[focused]).validate().unwrap();

    let mut stray = state();
    stray.focused_surface = Some(Uuid::new_v4().to_string().to_uppercase());
    assert_eq!(snapshot_of(&[stray]).validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn total_state_bytes_are_bounded() {
    let mut snapshot = snapshot();
    snapshot.tabs[0].state_data = vec![b' '; MAXIMUM_BYTES + 1];
    assert_eq!(snapshot.validate(), Err(RecoveryError::InvalidHistory));
}

#[test]
fn recovery_remaps_every_identity_consistently() {
    let a = SurfaceState { pwd: Some("/definitely/missing".into()), ..surface() };
    let b = SurfaceState { pwd: Some("/tmp".into()), ..surface() };
    let tree = SplitTree::with_view(a.clone()).inserting(b.clone(), &a, NewDirection::Right).unwrap();
    let group = GroupSnapshot {
        id: Uuid::new_v4(),
        info: GroupInfo { name: "Work".into(), color: TerminalTabColor::Purple, is_collapsed: false },
    };
    let state = RestorableState {
        focused_surface: Some(b.id_string()),
        sidebar_group: Some(group.clone()),
        tab_color: Some(TerminalTabColor::Purple),
        title_override: Some("Work".into()),
        ..RestorableState::new(tree)
    };

    let mut context = ClosedWindowRecoveryContext::new();
    let recovered = context.recover(state, |path| path == "/tmp");

    let views = recovered.surface_tree.views();
    assert_ne!(views[0].uuid, a.uuid);
    assert_ne!(views[1].uuid, b.uuid);
    assert_eq!(views[0].pwd, None);
    assert_eq!(views[1].pwd.as_deref(), Some("/tmp"));
    assert!(context.used_directory_fallback);
    assert_eq!(recovered.focused_surface, Some(views[1].id_string()));
    let recovered_group = recovered.sidebar_group.unwrap();
    assert_ne!(recovered_group.id, group.id);
    assert_eq!(recovered_group.info, group.info);
    assert_eq!(recovered.tab_color, Some(TerminalTabColor::Purple));
    assert_eq!(recovered.title_override.as_deref(), Some("Work"));

    let id = Uuid::new_v4();
    assert_eq!(context.replacement(id), context.replacement(id));
}

#[test]
fn the_same_group_in_two_tabs_maps_to_one_group() {
    let group = GroupSnapshot {
        id: Uuid::new_v4(),
        info: GroupInfo { name: "Work".into(), color: TerminalTabColor::Blue, is_collapsed: false },
    };
    let tab = || RestorableState { sidebar_group: Some(group.clone()), ..state() };
    let mut context = ClosedWindowRecoveryContext::new();
    let first = context.recover(tab(), |_| true);
    let second = context.recover(tab(), |_| true);
    assert_eq!(first.sidebar_group.unwrap().id, second.sidebar_group.unwrap().id);
    assert!(!context.used_directory_fallback);
}

#[test]
fn a_surface_without_a_directory_does_not_trigger_the_fallback() {
    let mut context = ClosedWindowRecoveryContext::new();
    context.recover(state(), |_| false);
    assert!(!context.used_directory_fallback);
}

#[test]
fn error_messages_match_the_app() {
    assert_eq!(
        RecoveryError::Unavailable.to_string(),
        "This window contains a terminal that cannot be saved for recovery."
    );
    assert_eq!(RecoveryError::Disabled.to_string(), "Window recovery is disabled.");
}
