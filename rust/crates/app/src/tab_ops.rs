//! Pure tab-order and tab-group rules. Every operation edits a list of tabs in place so
//! the workspace and the tests share one implementation.

use std::ops::Range;

use angelly_core::config::WindowNewTabPosition;
use angelly_core::persist::GroupLanding;
use uuid::Uuid;

pub trait Grouped {
    fn group(&self) -> Option<Uuid>;
    fn set_group(&mut self, group: Option<Uuid>);
}

/// Which side of a target a dragged tab lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Before,
    After,
}

impl Edge {
    /// The edge a fraction (0 to 1) along a target selects: the first half is before it.
    pub fn at(position: f32) -> Self {
        if position < 0.5 {
            Edge::Before
        } else {
            Edge::After
        }
    }
}

/// The side of `to` a tab dragged from `from` lands on, following the direction of travel.
/// `None` when it is dropped on itself.
pub fn inferred_edge(from: usize, to: usize) -> Option<Edge> {
    match from.cmp(&to) {
        std::cmp::Ordering::Less => Some(Edge::After),
        std::cmp::Ordering::Greater => Some(Edge::Before),
        std::cmp::Ordering::Equal => None,
    }
}

/// Moves the tab at `from` next to the tab at `target` and returns its new index.
fn move_next_to<T>(tabs: &mut Vec<T>, from: usize, target: usize, edge: Edge) -> usize {
    let tab = tabs.remove(from);
    let target = if from < target { target - 1 } else { target };
    let at = match edge {
        Edge::Before => target,
        Edge::After => target + 1,
    };
    tabs.insert(at, tab);
    at
}

/// The consecutive tabs of the group `anchor` belongs to, starting at `anchor`.
pub fn run_from<T: Grouped>(tabs: &[T], anchor: usize) -> Range<usize> {
    let Some(group) = tabs.get(anchor).and_then(Grouped::group) else {
        return anchor..anchor;
    };
    let len = tabs[anchor..].iter().take_while(|t| t.group() == Some(group)).count();
    anchor..anchor + len
}

fn last_member<T: Grouped>(tabs: &[T], group: Uuid, except: usize) -> Option<usize> {
    (0..tabs.len()).rev().find(|&i| i != except && tabs[i].group() == Some(group))
}

/// Drops the tab at `from` onto the tab at `to`: it lands beside it and joins its group
/// (or leaves its own). Returns the new index, `None` when nothing can drop there.
pub fn drop_on_tab<T: Grouped>(tabs: &mut Vec<T>, from: usize, to: usize, edge: Option<Edge>) -> Option<usize> {
    let inferred = inferred_edge(from, to)?;
    let group = tabs[to].group();
    let at = move_next_to(tabs, from, to, edge.unwrap_or(inferred));
    tabs[at].set_group(group);
    Some(at)
}

/// Drops the group run starting at `anchor` beside the tab at `to`, outside the whole run
/// of the target's group. Returns the run's new range.
pub fn drop_group_on_tab<T: Grouped>(
    tabs: &mut Vec<T>,
    anchor: usize,
    to: usize,
    edge: Option<Edge>,
) -> Option<Range<usize>> {
    let run = run_from(tabs, anchor);
    if run.is_empty() || run.contains(&to) {
        return None;
    }
    let edge = edge.unwrap_or(if run.start < to { Edge::After } else { Edge::Before });

    let mut target = to;
    if let Some(group) = tabs[to].group() {
        match edge {
            Edge::Before => {
                while target > 0 && tabs[target - 1].group() == Some(group) {
                    target -= 1;
                }
            }
            Edge::After => {
                while target + 1 < tabs.len() && tabs[target + 1].group() == Some(group) {
                    target += 1;
                }
            }
        }
    }

    let moving: Vec<T> = tabs.drain(run.clone()).collect();
    let target = if run.start < target { target - run.len() } else { target };
    let at = match edge {
        Edge::Before => target,
        Edge::After => target + 1,
    };
    let count = moving.len();
    tabs.splice(at..at, moving);
    Some(at..at + count)
}

/// Whether a tab can be dropped onto a group header to join it.
pub fn can_join(tab_group: Option<Uuid>, group: Uuid) -> bool {
    tab_group != Some(group)
}

/// Moves the tab at `index` behind the last tab of `group` and joins it, so a group stays
/// one run of tabs. Returns the new index.
pub fn join_group<T: Grouped>(tabs: &mut Vec<T>, index: usize, group: Uuid) -> Option<usize> {
    if tabs[index].group() == Some(group) {
        return None;
    }
    let last = last_member(tabs, group, index)?;
    let at = move_next_to(tabs, index, last, Edge::After);
    tabs[at].set_group(Some(group));
    Some(at)
}

/// Takes the tab out of its group. A tab leaving from the middle steps out behind the
/// group instead of splitting it. Returns the new index.
pub fn leave_group<T: Grouped>(tabs: &mut Vec<T>, index: usize) -> usize {
    let Some(group) = tabs[index].group() else { return index };
    let mut at = index;
    if let Some(last) = last_member(tabs, group, index) {
        if last > index {
            at = move_next_to(tabs, index, last, Edge::After);
        }
    }
    tabs[at].set_group(None);
    at
}

pub fn ungroup<T: Grouped>(tabs: &mut [T], group: Uuid) {
    for tab in tabs.iter_mut().filter(|t| t.group() == Some(group)) {
        tab.set_group(None);
    }
}

/// Drops the tab at `index` beside `group`, outside its run, ungrouped. Returns the new index.
pub fn drop_beside_group<T: Grouped>(tabs: &mut Vec<T>, index: usize, group: Uuid, edge: Edge) -> usize {
    let members = tabs.iter().enumerate().filter(|(i, t)| *i != index && t.group() == Some(group));
    let anchor = match edge {
        Edge::Before => members.map(|(i, _)| i).next(),
        Edge::After => members.map(|(i, _)| i).last(),
    };
    let at = match anchor {
        Some(anchor) => move_next_to(tabs, index, anchor, edge),
        None => index,
    };
    tabs[at].set_group(None);
    at
}

/// Dropping on a group header: beside the group in its outer quarters, inside it between.
pub fn drop_on_group<T: Grouped>(tabs: &mut Vec<T>, index: usize, group: Uuid, landing: GroupLanding) -> Option<usize> {
    match landing {
        GroupLanding::Before => Some(drop_beside_group(tabs, index, group, Edge::Before)),
        GroupLanding::After => Some(drop_beside_group(tabs, index, group, Edge::After)),
        GroupLanding::Inside => join_group(tabs, index, group),
    }
}

/// Appends the tab at `index` as the last, ungrouped tab. Returns the new index.
pub fn drop_tab_at_end<T: Grouped>(tabs: &mut Vec<T>, index: usize) -> usize {
    let last = tabs.len() - 1;
    let at = if index == last { index } else { move_next_to(tabs, index, last, Edge::After) };
    tabs[at].set_group(None);
    at
}

/// Appends the group run starting at `anchor` after the last tab.
pub fn drop_group_at_end<T: Grouped>(tabs: &mut Vec<T>, anchor: usize) -> Option<Range<usize>> {
    let last = tabs.len() - 1;
    drop_group_on_tab(tabs, anchor, last, Some(Edge::After))
}

/// Where a dragged tab lands on a group header, by how far along it the pointer is (0 to 1):
/// beside the group in the outer quarters, inside it in between. An expanded group's far
/// side leads into its own tabs, so only a collapsed group accepts a tab after it.
pub fn group_landing(collapsed: bool, position: f32) -> GroupLanding {
    if position < 0.25 {
        GroupLanding::Before
    } else if position > 0.75 && collapsed {
        GroupLanding::After
    } else {
        GroupLanding::Inside
    }
}

/// What is being dragged, by tab index: a tab, or the group run starting at an anchor tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drag {
    Tab(usize),
    Group(usize),
}

/// What a drag hovers over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over {
    Tab(usize),
    /// A group header whose run starts at `anchor`; `landing` is where a dragged tab
    /// would land on it at the current pointer position.
    GroupHeader { anchor: usize, group: Uuid, landing: GroupLanding },
    End,
}

/// What a hovering drag shows on its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feedback {
    None,
    /// A drop indicator on this side of the target.
    Edge(Edge),
    /// The dragged tab would join the hovered group.
    Join,
}

/// The feedback for a drag at `position` (0 to 1) along its target. The vertical sidebar
/// lands a drop on the side the drag came from; the horizontal bar by which half it hovers.
pub fn feedback<T: Grouped>(tabs: &[T], drag: Drag, over: Over, position: f32, horizontal: bool) -> Feedback {
    let by_position = || horizontal.then(|| Edge::at(position));
    let group_edge = |anchor: usize, to: usize| {
        let run = run_from(tabs, anchor);
        if run.is_empty() || run.contains(&to) {
            return Feedback::None;
        }
        Feedback::Edge(by_position().unwrap_or(if run.start < to { Edge::After } else { Edge::Before }))
    };
    match (drag, over) {
        (_, Over::End) => Feedback::Edge(Edge::Before),
        (Drag::Tab(from), Over::Tab(to)) => match inferred_edge(from, to) {
            Some(inferred) => Feedback::Edge(by_position().unwrap_or(inferred)),
            None => Feedback::None,
        },
        (Drag::Group(anchor), Over::Tab(to)) => group_edge(anchor, to),
        (Drag::Group(anchor), Over::GroupHeader { anchor: to, .. }) => group_edge(anchor, to),
        (Drag::Tab(from), Over::GroupHeader { group, landing, .. }) => match landing {
            GroupLanding::Before => Feedback::Edge(Edge::Before),
            GroupLanding::After => Feedback::Edge(Edge::After),
            GroupLanding::Inside if can_join(tabs[from].group(), group) => Feedback::Join,
            GroupLanding::Inside => Feedback::None,
        },
    }
}

/// Performs the drop that [`feedback`] previews. Returns whether the order or groups changed.
pub fn apply_drop<T: Grouped>(tabs: &mut Vec<T>, drag: Drag, over: Over, position: f32, horizontal: bool) -> bool {
    let edge = horizontal.then(|| Edge::at(position));
    match (drag, over) {
        (Drag::Tab(from), Over::Tab(to)) => drop_on_tab(tabs, from, to, edge).is_some(),
        (Drag::Group(anchor), Over::Tab(to)) => drop_group_on_tab(tabs, anchor, to, edge).is_some(),
        (Drag::Group(anchor), Over::GroupHeader { anchor: to, .. }) => {
            drop_group_on_tab(tabs, anchor, to, edge).is_some()
        }
        (Drag::Tab(from), Over::GroupHeader { group, landing, .. }) => {
            drop_on_group(tabs, from, group, landing).is_some()
        }
        (Drag::Tab(from), Over::End) => {
            drop_tab_at_end(tabs, from);
            true
        }
        (Drag::Group(anchor), Over::End) => drop_group_at_end(tabs, anchor).is_some(),
    }
}

/// Where a tab opened from `selected` goes, and the group it joins. A tab opened from
/// inside a group always joins it, so `End` means the end of that group's run.
pub fn new_tab_placement<T: Grouped>(tabs: &[T], selected: usize, position: WindowNewTabPosition) -> (usize, Option<Uuid>) {
    let group = tabs.get(selected).and_then(Grouped::group);
    let after = match position {
        WindowNewTabPosition::Current => selected,
        WindowNewTabPosition::End => tabs
            .iter()
            .rposition(|t| group.is_none() || t.group() == group)
            .unwrap_or(selected),
    };
    ((after + 1).min(tabs.len()), group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Tab(char, Option<Uuid>);

    impl Grouped for Tab {
        fn group(&self) -> Option<Uuid> {
            self.1
        }
        fn set_group(&mut self, group: Option<Uuid>) {
            self.1 = group;
        }
    }

    /// Uppercase letters belong to one group, lowercase ones are ungrouped.
    fn grouped(spec: &str) -> (Vec<Tab>, Uuid) {
        let group = Uuid::new_v4();
        let tabs = spec.chars().map(|c| Tab(c, c.is_uppercase().then_some(group))).collect();
        (tabs, group)
    }

    fn order(tabs: &[Tab]) -> String {
        tabs.iter().map(|t| t.0).collect()
    }

    #[test]
    fn edge_follows_direction_of_travel() {
        assert_eq!(inferred_edge(0, 2), Some(Edge::After));
        assert_eq!(inferred_edge(3, 1), Some(Edge::Before));
        assert_eq!(inferred_edge(2, 2), None);
        assert_eq!(Edge::at(0.49), Edge::Before);
        assert_eq!(Edge::at(0.5), Edge::After);
    }

    #[test]
    fn dropping_on_a_tab_lands_after_when_moving_forward() {
        let (mut t, _) = grouped("wxyz");
        assert_eq!(drop_on_tab(&mut t, 0, 2, None), Some(2));
        assert_eq!(order(&t), "xywz");
        assert_eq!(drop_on_tab(&mut t, 3, 0, None), Some(0));
        assert_eq!(order(&t), "zxyw");
        assert_eq!(drop_on_tab(&mut t, 1, 1, None), None);
    }

    #[test]
    fn explicit_edge_overrides_travel_direction() {
        let (mut t, _) = grouped("wxyz");
        assert_eq!(drop_on_tab(&mut t, 0, 2, Some(Edge::Before)), Some(1));
        assert_eq!(order(&t), "xwyz");
    }

    #[test]
    fn dropping_next_to_a_grouped_tab_joins_its_group() {
        let (mut t, g) = grouped("wXYz");
        assert_eq!(drop_on_tab(&mut t, 0, 1, None), Some(1));
        assert_eq!(order(&t), "XwYz");
        assert_eq!(t[1].group(), Some(g));
        assert_eq!(drop_on_tab(&mut t, 1, 3, None), Some(3));
        assert_eq!(t[3].group(), None);
    }

    #[test]
    fn dragging_a_group_lands_outside_the_target_run() {
        let (mut t, _) = grouped("ABy");
        let other = Uuid::new_v4();
        t.extend([Tab('C', Some(other)), Tab('D', Some(other))]);
        assert_eq!(drop_group_on_tab(&mut t, 0, 3, None), Some(3..5));
        assert_eq!(order(&t), "yCDAB");
        assert_eq!(drop_group_on_tab(&mut t, 3, 0, None), Some(0..2));
        assert_eq!(order(&t), "ABy".to_string() + "CD");
    }

    #[test]
    fn a_group_cannot_drop_on_itself() {
        let (mut t, _) = grouped("ABy");
        assert_eq!(drop_group_on_tab(&mut t, 0, 1, None), None);
    }

    #[test]
    fn joining_a_group_moves_behind_its_last_tab() {
        let (mut t, g) = grouped("xABy");
        assert_eq!(join_group(&mut t, 0, g), Some(2));
        assert_eq!(order(&t), "ABxy");
        assert_eq!(t[2].group(), Some(g));
        assert_eq!(join_group(&mut t, 2, g), None);
    }

    #[test]
    fn joining_an_otherwise_empty_group_does_nothing() {
        let g = Uuid::new_v4();
        let mut t = vec![Tab('a', Some(g)), Tab('b', None)];
        assert_eq!(join_group(&mut t, 1, g), Some(1));
        let mut lone = vec![Tab('a', None), Tab('b', Some(g))];
        assert_eq!(join_group(&mut lone, 1, g), None);
    }

    #[test]
    fn leaving_from_the_middle_steps_out_behind_the_group() {
        let (mut t, _) = grouped("ABCy");
        assert_eq!(leave_group(&mut t, 0), 2);
        assert_eq!(order(&t), "BCAy");
        assert_eq!(t[2].group(), None);
        assert_eq!(leave_group(&mut t, 1), 1);
        assert_eq!(order(&t), "BCAy");
    }

    #[test]
    fn ungroup_clears_every_member() {
        let (mut t, g) = grouped("ABy");
        ungroup(&mut t, g);
        assert!(t.iter().all(|t| t.group().is_none()));
    }

    #[test]
    fn dropping_beside_a_group_ungroups_and_lands_outside_it() {
        let (mut t, g) = grouped("xABC");
        assert_eq!(drop_beside_group(&mut t, 2, g, Edge::After), 3);
        assert_eq!(order(&t), "xACB");
        assert_eq!(t[3].group(), None);
        let (mut t, g) = grouped("ABCx");
        assert_eq!(drop_beside_group(&mut t, 1, g, Edge::Before), 0);
        assert_eq!(order(&t), "BACx");
        assert_eq!(t[0].group(), None);
    }

    #[test]
    fn group_header_landing_picks_the_action() {
        let (mut t, g) = grouped("xAB");
        assert_eq!(drop_on_group(&mut t, 0, g, GroupLanding::Inside), Some(2));
        assert_eq!(order(&t), "ABx");
        assert_eq!(t[2].group(), Some(g));
        assert!(!can_join(Some(g), g));
        assert!(can_join(None, g));
    }

    #[test]
    fn end_drop_appends_ungrouped() {
        let (mut t, _) = grouped("ABxy");
        assert_eq!(drop_tab_at_end(&mut t, 0), 3);
        assert_eq!(order(&t), "BxyA");
        assert_eq!(t[3].group(), None);
        let (mut t, _) = grouped("ABxy");
        assert_eq!(drop_group_at_end(&mut t, 0), Some(2..4));
        assert_eq!(order(&t), "xyAB");
    }

    #[test]
    fn new_tab_placement_respects_groups_and_position() {
        let (t, g) = grouped("xAByz");
        assert_eq!(new_tab_placement(&t, 1, WindowNewTabPosition::Current), (2, Some(g)));
        assert_eq!(new_tab_placement(&t, 1, WindowNewTabPosition::End), (3, Some(g)));
        assert_eq!(new_tab_placement(&t, 0, WindowNewTabPosition::Current), (1, None));
        assert_eq!(new_tab_placement(&t, 0, WindowNewTabPosition::End), (5, None));
    }

    #[test]
    fn landing_depends_on_position_and_collapse() {
        assert_eq!(group_landing(false, 0.1), GroupLanding::Before);
        assert_eq!(group_landing(false, 0.9), GroupLanding::Inside);
        assert_eq!(group_landing(true, 0.9), GroupLanding::After);
        assert_eq!(group_landing(true, 0.5), GroupLanding::Inside);
    }

    #[test]
    fn feedback_previews_what_a_drop_does() {
        let (t, g) = grouped("wAByz");
        let header = |landing| Over::GroupHeader { anchor: 1, group: g, landing };
        assert_eq!(feedback(&t, Drag::Tab(0), Over::Tab(4), 0.1, false), Feedback::Edge(Edge::After));
        assert_eq!(feedback(&t, Drag::Tab(4), Over::Tab(0), 0.9, false), Feedback::Edge(Edge::Before));
        assert_eq!(feedback(&t, Drag::Tab(4), Over::Tab(0), 0.9, true), Feedback::Edge(Edge::After));
        assert_eq!(feedback(&t, Drag::Tab(2), Over::Tab(2), 0.5, false), Feedback::None);
        assert_eq!(feedback(&t, Drag::Tab(0), header(GroupLanding::Inside), 0.5, false), Feedback::Join);
        assert_eq!(feedback(&t, Drag::Tab(2), header(GroupLanding::Inside), 0.5, false), Feedback::None);
        assert_eq!(feedback(&t, Drag::Tab(0), header(GroupLanding::Before), 0.1, false), Feedback::Edge(Edge::Before));
        assert_eq!(feedback(&t, Drag::Group(1), Over::Tab(2), 0.5, false), Feedback::None);
        assert_eq!(feedback(&t, Drag::Group(1), Over::Tab(4), 0.5, false), Feedback::Edge(Edge::After));
        assert_eq!(feedback(&t, Drag::Tab(0), Over::End, 0.5, false), Feedback::Edge(Edge::Before));
    }

    #[test]
    fn apply_drop_dispatches_by_target() {
        let (mut t, g) = grouped("wAByz");
        let landing = GroupLanding::Inside;
        assert!(apply_drop(&mut t, Drag::Tab(0), Over::GroupHeader { anchor: 0, group: g, landing }, 0.5, false));
        assert_eq!(order(&t), "ABwyz");
        assert!(apply_drop(&mut t, Drag::Group(0), Over::End, 0.5, false));
        assert_eq!(order(&t), "yzABw");
        assert!(apply_drop(&mut t, Drag::Tab(4), Over::Tab(0), 0.2, true));
        assert_eq!(order(&t), "wyzAB");
        assert_eq!(t[0].group(), None);
    }
}
