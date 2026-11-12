//! The state saved for a terminal window so it can be restored on launch or reopened.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::groups::GroupSnapshot;
use super::split_tree::SplitTree;
use super::tab::TerminalTabColor;

/// Version written by this app. Bump it, and add a migration test, when the layout changes.
pub const VERSION: i64 = 8;
/// Oldest version that can still be decoded.
pub const MINIMUM_VERSION: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FullscreenMode {
    Native,
    NonNative,
    NonNativeVisibleMenu,
    NonNativePaddedNotch,
}

/// A saved terminal surface, the leaf of a window's split tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceState {
    #[serde(with = "super::swift::uuid")]
    pub uuid: Uuid,
    /// Always written, as `null` when unknown.
    pub pwd: Option<String>,
    /// Always written, as `null` when unknown.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub is_user_set_title: bool,
}

impl SurfaceState {
    pub fn new(uuid: Uuid) -> Self {
        Self { uuid, pwd: None, title: None, is_user_set_title: false }
    }

    /// The uuid in the form the saved focus refers to it by.
    pub fn id_string(&self) -> String {
        super::swift::uuid::string(&self.uuid)
    }
}

/// Everything saved for one terminal window. `V` is the saved leaf type, usually [`SurfaceState`].
///
/// Absent fields come from older versions and decode as `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "camelCase",
    bound(serialize = "V: Clone + PartialEq + Serialize", deserialize = "V: Clone + PartialEq + Deserialize<'de>")
)]
pub struct RestorableState<V> {
    /// Version 5: the id of the focused surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_surface: Option<String>,
    pub surface_tree: SplitTree<V>,
    /// Version 7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_fullscreen_mode: Option<FullscreenMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_color: Option<TerminalTabColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_override: Option<String>,
    /// Version 8.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_group: Option<GroupSnapshot>,
}

impl<V: Clone + PartialEq> RestorableState<V> {
    pub fn new(surface_tree: SplitTree<V>) -> Self {
        Self {
            focused_surface: None,
            surface_tree,
            effective_fullscreen_mode: None,
            tab_color: None,
            title_override: None,
            sidebar_group: None,
        }
    }
}

/// Whether a stored state version can be decoded.
pub fn is_supported_version(version: i64) -> bool {
    (MINIMUM_VERSION..=VERSION).contains(&version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::groups::GroupInfo;
    use crate::persist::split_tree::NewDirection;

    fn surface(id: &str) -> SurfaceState {
        SurfaceState::new(Uuid::parse_str(id).unwrap())
    }

    fn two_surfaces() -> (SplitTree<SurfaceState>, SurfaceState, SurfaceState) {
        let a = surface("926F3F2A-824C-40C9-87CA-2CDCA4E11049");
        let b = surface("AC5E829B-85FD-4C69-B196-2EE469C72A90");
        let tree = SplitTree::with_view(a.clone())
            .inserting(b.clone(), &a, NewDirection::Right)
            .unwrap();
        (tree, a, b)
    }

    #[test]
    fn versions_are_guarded() {
        // Changing either number needs a migration test for the old layout.
        assert_eq!(VERSION, 8);
        assert_eq!(MINIMUM_VERSION, 5);
        assert!(is_supported_version(5) && is_supported_version(8));
        assert!(!is_supported_version(4) && !is_supported_version(9));
    }

    #[test]
    fn sidebar_group_round_trips() {
        let (tree, ..) = two_surfaces();
        let group = GroupSnapshot {
            id: Uuid::new_v4(),
            info: GroupInfo { name: "Work".into(), color: TerminalTabColor::Purple, is_collapsed: true },
        };
        let mut state = RestorableState::new(tree);
        state.sidebar_group = Some(group.clone());

        let decoded: RestorableState<SurfaceState> =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        let decoded_group = decoded.sidebar_group.unwrap();
        assert_eq!(decoded_group.id, group.id);
        assert_eq!(decoded_group.info.name, "Work");
        assert_eq!(decoded_group.info.color, TerminalTabColor::Purple);
        assert!(decoded_group.info.is_collapsed);
    }

    #[test]
    fn json_matches_the_swift_layout() {
        let (tree, ..) = two_surfaces();
        let mut state = RestorableState::new(tree);
        state.focused_surface = Some("AC5E829B-85FD-4C69-B196-2EE469C72A90".into());
        state.effective_fullscreen_mode = Some(FullscreenMode::NonNativePaddedNotch);
        state.tab_color = Some(TerminalTabColor::Green);
        state.title_override = Some("tip".into());

        let value = serde_json::to_value(&state).unwrap();
        assert_eq!(value["focusedSurface"], "AC5E829B-85FD-4C69-B196-2EE469C72A90");
        assert_eq!(value["effectiveFullscreenMode"], "nonNativePaddedNotch");
        assert_eq!(value["tabColor"], 7);
        assert_eq!(value["titleOverride"], "tip");
        assert!(value.get("sidebarGroup").is_none());
        let leaf = &value["surfaceTree"]["root"]["split"]["left"]["view"];
        assert_eq!(leaf["uuid"], "926F3F2A-824C-40C9-87CA-2CDCA4E11049");
        assert!(leaf["pwd"].is_null() && leaf.get("pwd").is_some());
        assert!(leaf["title"].is_null() && leaf.get("title").is_some());
        assert_eq!(leaf["isUserSetTitle"], false);
    }

    #[test]
    fn version_five_state_decodes_without_later_fields() {
        let json = r#"{
            "focusedSurface": "v5",
            "surfaceTree": {"version": 1, "root": {"split": {
                "direction": {"horizontal": {}}, "ratio": 0.5,
                "left": {"view": {"uuid": "926F3F2A-824C-40C9-87CA-2CDCA4E11049", "pwd": null}},
                "right": {"view": {"uuid": "AC5E829B-85FD-4C69-B196-2EE469C72A90", "pwd": "/tmp", "title": "zsh", "isUserSetTitle": true}}
            }}}
        }"#;
        let state: RestorableState<SurfaceState> = serde_json::from_str(json).unwrap();
        assert_eq!(state.focused_surface.as_deref(), Some("v5"));
        assert_eq!(state.effective_fullscreen_mode, None);
        assert_eq!(state.tab_color, None);
        assert_eq!(state.title_override, None);
        assert_eq!(state.sidebar_group, None);
        let views = state.surface_tree.views();
        assert_eq!(views.len(), 2);
        assert_eq!(views[1].pwd.as_deref(), Some("/tmp"));
        assert_eq!(views[1].title.as_deref(), Some("zsh"));
        assert!(views[1].is_user_set_title);
    }

    #[test]
    fn fullscreen_modes_use_their_swift_raw_values() {
        let raw = [
            (FullscreenMode::Native, "native"),
            (FullscreenMode::NonNative, "nonNative"),
            (FullscreenMode::NonNativeVisibleMenu, "nonNativeVisibleMenu"),
            (FullscreenMode::NonNativePaddedNotch, "nonNativePaddedNotch"),
        ];
        for (mode, text) in raw {
            assert_eq!(serde_json::to_string(&mode).unwrap(), format!("\"{text}\""));
            assert_eq!(serde_json::from_str::<FullscreenMode>(&format!("\"{text}\"")).unwrap(), mode);
        }
        assert!(serde_json::from_str::<FullscreenMode>("\"other\"").is_err());
    }

    #[test]
    fn a_state_without_a_surface_tree_is_rejected() {
        assert!(serde_json::from_str::<RestorableState<SurfaceState>>("{}").is_err());
    }
}
