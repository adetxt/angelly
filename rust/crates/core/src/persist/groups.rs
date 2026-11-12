//! Tab groups shown in the sidebar: shared group metadata and the row list built from tabs.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::tab::TerminalTabColor;
use crate::git::GitTabStatus;

/// Group metadata. Which tabs belong to a group is a property of each tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub name: String,
    pub color: TerminalTabColor,
    pub is_collapsed: bool,
}

/// A group with its identity, as saved with a restorable window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSnapshot {
    #[serde(with = "super::swift::uuid")]
    pub id: Uuid,
    pub info: GroupInfo,
}

const GROUP_PALETTE: [TerminalTabColor; 8] = [
    TerminalTabColor::Blue,
    TerminalTabColor::Purple,
    TerminalTabColor::Pink,
    TerminalTabColor::Red,
    TerminalTabColor::Orange,
    TerminalTabColor::Yellow,
    TerminalTabColor::Green,
    TerminalTabColor::Teal,
];

pub const DEFAULT_GROUP_NAME: &str = "Group";

/// App-wide group metadata. Groups are shared because every window of a tab group
/// renders its own bar.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabGroupStore {
    groups: HashMap<Uuid, GroupInfo>,
}

impl TabGroupStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn groups(&self) -> &HashMap<Uuid, GroupInfo> {
        &self.groups
    }

    pub fn get(&self, id: Uuid) -> Option<&GroupInfo> {
        self.groups.get(&id)
    }

    pub fn len(&self) -> usize {
        self.groups.len()
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn snapshot(&self, id: Option<Uuid>) -> Option<GroupSnapshot> {
        let id = id?;
        self.groups.get(&id).map(|info| GroupSnapshot { id, info: info.clone() })
    }

    /// Adds a saved group unless one with that id already exists.
    pub fn restore(&mut self, snapshot: GroupSnapshot) {
        self.groups.entry(snapshot.id).or_insert(snapshot.info);
    }

    /// Adds a group, coloring it with the next palette color so neighbors look different.
    pub fn create(&mut self) -> Uuid {
        let id = Uuid::new_v4();
        let color = GROUP_PALETTE[self.groups.len() % GROUP_PALETTE.len()];
        self.groups.insert(
            id,
            GroupInfo { name: DEFAULT_GROUP_NAME.to_string(), color, is_collapsed: false },
        );
        id
    }

    /// Applies `change` to a group. Returns whether the group exists.
    pub fn update(&mut self, id: Uuid, change: impl FnOnce(&mut GroupInfo)) -> bool {
        self.groups.get_mut(&id).map(change).is_some()
    }

    pub fn remove(&mut self, id: Uuid) {
        self.groups.remove(&id);
    }

    /// Drops groups no tab refers to. Returns the removed ids.
    pub fn prune(&mut self, live: &HashSet<Uuid>) -> Vec<Uuid> {
        let stale: Vec<Uuid> = self.groups.keys().filter(|id| !live.contains(id)).copied().collect();
        for id in &stale {
            self.groups.remove(id);
        }
        stale
    }

    /// Renames a group. Blank names are ignored. Returns whether the name changed.
    pub fn rename(&mut self, id: Uuid, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.update(id, |info| info.name = name.to_string())
    }
}

/// One tab as the sidebar lists it. `T` identifies the tab.
#[derive(Debug, Clone, PartialEq)]
pub struct TabItem<T> {
    pub id: T,
    pub title: String,
    /// The tab's own color; grouped tabs show their group's color instead.
    pub color: Option<TerminalTabColor>,
    pub shortcut: Option<String>,
    pub has_bell: bool,
    pub is_selected: bool,
    /// `None` when the tab is ungrouped.
    pub group: Option<Uuid>,
    pub group_color: Option<TerminalTabColor>,
    pub git_status: Option<GitTabStatus>,
}

impl<T> TabItem<T> {
    /// Tooltip text: the title with its shortcut, then the git detail.
    pub fn detail(&self) -> String {
        let heading = match &self.shortcut {
            Some(shortcut) => format!("{} ({shortcut})", self.title),
            None => self.title.clone(),
        };
        match &self.git_status {
            Some(status) => format!("{heading}\n{}", status.detail()),
            None => heading,
        }
    }
}

/// A run of consecutive tabs of one group, shown as a collapsible header above them.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupHeader {
    pub id: Uuid,
    pub name: String,
    pub color: Option<TerminalTabColor>,
    pub is_collapsed: bool,
    pub count: usize,
    pub has_bell: bool,
    pub contains_selected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RowId<T> {
    Tab(T),
    /// Anchored on its first tab so a group split apart by a tab move keeps unique rows.
    Group(Uuid, T),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Row<T> {
    Group { header: GroupHeader, anchor: T },
    Tab(TabItem<T>),
}

impl<T: Clone> Row<T> {
    pub fn id(&self) -> RowId<T> {
        match self {
            Row::Group { header, anchor } => RowId::Group(header.id, anchor.clone()),
            Row::Tab(item) => RowId::Tab(item.id.clone()),
        }
    }
}

/// What the sidebar lists: the items with group headers, minus the tabs of collapsed groups.
pub fn rows<T: Clone>(items: &[TabItem<T>], groups: &HashMap<Uuid, GroupInfo>) -> Vec<Row<T>> {
    let mut rows = Vec::new();
    let mut index = 0;
    while index < items.len() {
        let item = &items[index];
        let Some((id, info)) = item.group.and_then(|id| groups.get(&id).map(|info| (id, info))) else {
            rows.push(Row::Tab(item.clone()));
            index += 1;
            continue;
        };

        let run: Vec<&TabItem<T>> = items[index..].iter().take_while(|i| i.group == Some(id)).collect();
        rows.push(Row::Group {
            header: GroupHeader {
                id,
                name: info.name.clone(),
                color: run[0].group_color,
                is_collapsed: info.is_collapsed,
                count: run.len(),
                has_bell: run.iter().any(|i| i.has_bell),
                contains_selected: run.iter().any(|i| i.is_selected),
            },
            anchor: item.id.clone(),
        });
        if !info.is_collapsed {
            rows.extend(run.iter().map(|i| Row::Tab((*i).clone())));
        }
        index += run.len();
    }
    rows
}

/// The row to highlight: the selected tab, or the header of a collapsed group holding it.
pub fn selected_row_id<T: Clone>(rows: &[Row<T>]) -> Option<RowId<T>> {
    rows.iter()
        .find(|row| match row {
            Row::Tab(item) => item.is_selected,
            Row::Group { header, .. } => header.is_collapsed && header.contains_selected,
        })
        .map(Row::id)
}

/// One header per group, even when a group is split into several runs.
pub fn group_headers<T>(rows: &[Row<T>]) -> Vec<&GroupHeader> {
    let mut seen = HashSet::new();
    rows.iter()
        .filter_map(|row| match row {
            Row::Group { header, .. } if seen.insert(header.id) => Some(header),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupLanding {
    Before,
    After,
    Inside,
}

/// Where a dragged tab lands on a group header, by how far along it the drag is (0 to 1):
/// beside the group in the outer quarters, inside it in between. An expanded group's far
/// side leads into its own tabs, so only a collapsed group accepts a tab after it.
pub fn landing(header: &GroupHeader, position: f64) -> GroupLanding {
    if position < 0.25 {
        GroupLanding::Before
    } else if position > 0.75 && header.is_collapsed {
        GroupLanding::After
    } else {
        GroupLanding::Inside
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u32, group: Option<Uuid>) -> TabItem<u32> {
        TabItem {
            id,
            title: format!("tab {id}"),
            color: None,
            shortcut: None,
            has_bell: false,
            is_selected: false,
            group,
            group_color: group.map(|_| TerminalTabColor::Purple),
            git_status: None,
        }
    }

    fn store_with(id: Uuid, collapsed: bool) -> TabGroupStore {
        let mut store = TabGroupStore::new();
        store.restore(GroupSnapshot {
            id,
            info: GroupInfo { name: "Work".into(), color: TerminalTabColor::Purple, is_collapsed: collapsed },
        });
        store
    }

    #[test]
    fn created_groups_rotate_through_the_palette() {
        let mut store = TabGroupStore::new();
        let colors: Vec<_> = (0..9)
            .map(|_| {
                let id = store.create();
                store.get(id).unwrap().color
            })
            .collect();
        assert_eq!(&colors[..8], &GROUP_PALETTE);
        assert_eq!(colors[8], TerminalTabColor::Blue);
        let id = store.create();
        let info = store.get(id).unwrap();
        assert_eq!(info.name, "Group");
        assert!(!info.is_collapsed);
    }

    #[test]
    fn restore_keeps_an_existing_group() {
        let id = Uuid::new_v4();
        let mut store = store_with(id, false);
        store.restore(GroupSnapshot {
            id,
            info: GroupInfo { name: "Other".into(), color: TerminalTabColor::Red, is_collapsed: true },
        });
        assert_eq!(store.get(id).unwrap().name, "Work");
        assert_eq!(store.snapshot(Some(id)).unwrap().info.name, "Work");
        assert!(store.snapshot(None).is_none());
        assert!(store.snapshot(Some(Uuid::new_v4())).is_none());
    }

    #[test]
    fn prune_drops_groups_without_tabs() {
        let mut store = TabGroupStore::new();
        let kept = store.create();
        let dropped = store.create();
        let removed = store.prune(&HashSet::from([kept]));
        assert_eq!(removed, vec![dropped]);
        assert_eq!(store.len(), 1);
        assert!(store.get(kept).is_some());
        store.remove(kept);
        assert!(store.is_empty());
    }

    #[test]
    fn update_and_rename() {
        let mut store = TabGroupStore::new();
        let id = store.create();
        assert!(store.update(id, |info| info.is_collapsed = !info.is_collapsed));
        assert!(store.get(id).unwrap().is_collapsed);
        assert!(!store.update(Uuid::new_v4(), |_| {}));

        assert!(store.rename(id, "  Docs \n"));
        assert_eq!(store.get(id).unwrap().name, "Docs");
        assert!(!store.rename(id, "   "));
        assert_eq!(store.get(id).unwrap().name, "Docs");
        assert!(!store.rename(Uuid::new_v4(), "x"));
    }

    #[test]
    fn rows_add_a_header_per_group_run() {
        let group = Uuid::new_v4();
        let store = store_with(group, false);
        let items = [item(0, None), item(1, Some(group)), item(2, Some(group)), item(3, None)];
        let rows = rows(&items, store.groups());
        assert_eq!(rows.len(), 5);
        let Row::Group { header, anchor } = &rows[1] else { panic!("expected a header") };
        assert_eq!((header.id, header.count, *anchor), (group, 2, 1));
        assert_eq!(header.name, "Work");
        assert_eq!(header.color, Some(TerminalTabColor::Purple));
        assert_eq!(rows[1].id(), RowId::Group(group, 1));
        assert_eq!(rows[2].id(), RowId::Tab(1));
    }

    #[test]
    fn collapsed_groups_hide_their_tabs_but_keep_the_selection() {
        let group = Uuid::new_v4();
        let store = store_with(group, true);
        let mut items = [item(0, Some(group)), item(1, Some(group)), item(2, None)];
        items[0].is_selected = true;
        items[1].has_bell = true;
        let rows = rows(&items, store.groups());
        assert_eq!(rows.len(), 2);
        let headers = group_headers(&rows);
        assert_eq!(headers.len(), 1);
        assert!(headers[0].is_collapsed && headers[0].contains_selected && headers[0].has_bell);
        assert_eq!(headers[0].count, 2);
        assert_eq!(selected_row_id(&rows), Some(RowId::Group(group, 0)));
    }

    #[test]
    fn tabs_of_an_unknown_group_are_listed_ungrouped() {
        let rows = rows(&[item(0, Some(Uuid::new_v4()))], &HashMap::new());
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], Row::Tab(_)));
    }

    #[test]
    fn a_group_split_apart_keeps_unique_rows() {
        let group = Uuid::new_v4();
        let store = store_with(group, false);
        let items = [item(0, Some(group)), item(1, None), item(2, Some(group))];
        let rows = rows(&items, store.groups());
        let ids: Vec<_> = rows.iter().map(Row::id).collect();
        assert_eq!(ids.len(), 5);
        for (i, id) in ids.iter().enumerate() {
            assert!(!ids[..i].contains(id));
        }
        assert_eq!(group_headers(&rows).len(), 1);
    }

    #[test]
    fn selected_tab_row_is_found() {
        let mut items = [item(0, None), item(1, None)];
        items[1].is_selected = true;
        let rows = rows(&items, &HashMap::new());
        assert_eq!(selected_row_id(&rows), Some(RowId::Tab(1)));
    }

    #[test]
    fn tab_lands_beside_a_group_only_at_its_outer_quarters() {
        let group = Uuid::new_v4();
        let store = store_with(group, false);
        let items = [item(0, Some(group))];
        let expanded = rows(&items, store.groups());
        let expanded = group_headers(&expanded)[0].clone();
        assert_eq!(landing(&expanded, 0.1), GroupLanding::Before);
        assert_eq!(landing(&expanded, 0.5), GroupLanding::Inside);
        assert_eq!(landing(&expanded, 0.9), GroupLanding::Inside);

        let collapsed = GroupHeader { is_collapsed: true, ..expanded };
        assert_eq!(landing(&collapsed, 0.5), GroupLanding::Inside);
        assert_eq!(landing(&collapsed, 0.9), GroupLanding::After);
    }

    #[test]
    fn item_detail_includes_the_shortcut() {
        let mut tab = item(0, None);
        assert_eq!(tab.detail(), "tab 0");
        tab.shortcut = Some("⌘1".into());
        assert_eq!(tab.detail(), "tab 0 (⌘1)");
    }

    #[test]
    fn group_json_matches_the_swift_layout() {
        let id = Uuid::parse_str("2f2f2d93-944c-474a-83ba-4dc1868c3eb9").unwrap();
        let snapshot = GroupSnapshot {
            id,
            info: GroupInfo { name: "Work".into(), color: TerminalTabColor::Purple, is_collapsed: true },
        };
        let value = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "id": "2F2F2D93-944C-474A-83BA-4DC1868C3EB9",
                "info": {"name": "Work", "color": 2, "isCollapsed": true},
            })
        );
        assert_eq!(serde_json::from_value::<GroupSnapshot>(value).unwrap(), snapshot);
    }
}
