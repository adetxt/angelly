use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::*;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MockView {
    id: u64,
    #[serde(skip)]
    size: (f64, f64),
}

impl PartialEq for MockView {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl MockView {
    fn new() -> Self {
        Self { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), size: (0.0, 0.0) }
    }

    fn sized(width: f64, height: f64) -> Self {
        Self { size: (width, height), ..Self::new() }
    }

    fn leaf(&self) -> Node<MockView> {
        Node::Leaf(self.clone())
    }
}

type Tree = SplitTree<MockView>;

fn horizontal_split() -> (Tree, MockView, MockView) {
    let (a, b) = (MockView::new(), MockView::new());
    let tree = Tree::with_view(a.clone()).inserting(b.clone(), &a, NewDirection::Right).unwrap();
    (tree, a, b)
}

fn three_across() -> (Tree, MockView, MockView, MockView) {
    let (a, b, c) = (MockView::new(), MockView::new(), MockView::new());
    let tree = Tree::with_view(a.clone())
        .inserting(b.clone(), &a, NewDirection::Right)
        .unwrap()
        .inserting(c.clone(), &b, NewDirection::Right)
        .unwrap();
    (tree, a, b, c)
}

fn grid() -> (Tree, [MockView; 4]) {
    let views = [MockView::new(), MockView::new(), MockView::new(), MockView::new()];
    let [a, b, c, d] = &views;
    let tree = Tree::with_view(a.clone())
        .inserting(b.clone(), a, NewDirection::Right)
        .unwrap()
        .inserting(c.clone(), a, NewDirection::Down)
        .unwrap()
        .inserting(d.clone(), b, NewDirection::Down)
        .unwrap();
    (tree, views)
}

fn size_of(view: &MockView) -> Size {
    Size::new(view.size.0, view.size.1)
}

fn root_split(tree: &Tree) -> &Split<MockView> {
    match tree.root() {
        Some(Node::Split(split)) => split,
        other => panic!("expected a split, got {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.001
}

#[test]
fn empty_and_single_trees() {
    assert!(Tree::default().is_empty());
    let tree = Tree::with_view(MockView::new());
    assert!(!tree.is_empty());
    assert!(!tree.is_split());
    assert!(horizontal_split().0.is_split());
}

#[test]
fn contains_and_find() {
    let view = MockView::new();
    assert!(Tree::with_view(view.clone()).contains(&view.leaf()));
    assert!(!Tree::default().contains(&view.leaf()));

    let (tree, a, _) = horizontal_split();
    let id = a.id;
    assert!(tree.find_by(|v| v.id == id).is_some());
    let other = MockView::new();
    assert!(Tree::with_view(a).find_by(|v| v.id == other.id).is_none());
}

#[test]
fn removing_a_view() {
    let (tree, a, b) = horizontal_split();
    let tree = tree.removing(&a.leaf());
    assert!(!tree.contains(&a.leaf()));
    assert!(tree.contains(&b.leaf()));
}

#[test]
fn removing_a_missing_node_changes_nothing() {
    let (a, b) = (MockView::new(), MockView::new());
    let result = Tree::with_view(a.clone()).removing(&b.leaf());
    assert!(result.contains(&a.leaf()));
    assert!(!result.is_empty());
}

#[test]
fn replacing_a_view() {
    let (tree, a, b) = horizontal_split();
    let c = MockView::new();
    let result = tree.replacing(&b.leaf(), c.leaf()).unwrap();
    assert!(result.contains(&a.leaf()));
    assert!(!result.contains(&b.leaf()));
    assert!(result.contains(&c.leaf()));
}

#[test]
fn replacing_a_view_with_itself_is_valid() {
    let (tree, a, b) = horizontal_split();
    let result = tree.replacing(&b.leaf(), b.leaf()).unwrap();
    assert!(result.contains(&a.leaf()));
    assert!(result.contains(&b.leaf()));
}

#[test]
fn replacing_a_missing_node_fails() {
    let (tree, _, _) = horizontal_split();
    let missing = MockView::new();
    assert_eq!(tree.replacing(&missing.leaf(), missing.leaf()), Err(SplitError::ViewNotFound));
}

#[test]
fn inserting_next_to_a_missing_view_fails() {
    let (a, missing) = (MockView::new(), MockView::new());
    let tree = Tree::with_view(a);
    assert_eq!(
        tree.inserting(MockView::new(), &missing, NewDirection::Left),
        Err(SplitError::ViewNotFound)
    );
    assert_eq!(
        Tree::default().inserting(MockView::new(), &missing, NewDirection::Left),
        Err(SplitError::ViewNotFound)
    );
}

#[test]
fn inserting_left_and_up_puts_the_new_view_first() {
    let a = MockView::new();
    for (direction, expected) in [
        (NewDirection::Left, Direction::Horizontal),
        (NewDirection::Up, Direction::Vertical),
    ] {
        let b = MockView::new();
        let tree = Tree::with_view(a.clone()).inserting(b.clone(), &a, direction).unwrap();
        let split = root_split(&tree);
        assert_eq!(split.direction, expected);
        assert_eq!(split.ratio, 0.5);
        assert_eq!(*split.left, b.leaf());
        assert_eq!(*split.right, a.leaf());
    }
}

#[test]
fn inserting_clears_the_zoom() {
    let (tree, a, b, _) = three_across();
    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    assert!(zoomed.is_zoomed());
    let result = zoomed.inserting(MockView::new(), &a, NewDirection::Down).unwrap();
    assert!(result.zoomed().is_none());
}

#[test]
fn focus_target_on_empty_tree_is_none() {
    let view = MockView::new();
    assert_eq!(Tree::default().focus_target(FocusDirection::Next, &view.leaf()), None);
}

#[test]
fn focus_target_next_previous_and_wrapping() {
    let (tree, a, b) = horizontal_split();
    assert_eq!(tree.focus_target(FocusDirection::Next, &a.leaf()), Some(b.clone()));
    assert_eq!(tree.focus_target(FocusDirection::Next, &b.leaf()), Some(a.clone()));
    assert_eq!(tree.focus_target(FocusDirection::Previous, &b.leaf()), Some(a.clone()));
    assert_eq!(tree.focus_target(FocusDirection::Previous, &a.leaf()), Some(b));

    let only = MockView::new();
    let single = Tree::with_view(only.clone());
    assert_eq!(single.focus_target(FocusDirection::Next, &only.leaf()), Some(only));
}

#[test]
fn focus_target_spatial() {
    let (tree, a, b) = horizontal_split();
    let left = FocusDirection::Spatial(SpatialDirection::Left);
    assert_eq!(tree.focus_target(left, &b.leaf()), Some(a.clone()));
    assert_eq!(tree.focus_target(left, &a.leaf()), None);

    let (tree, [a, b, c, d]) = grid();
    let target = |direction, from: &MockView| tree.focus_target(FocusDirection::Spatial(direction), &from.leaf());
    assert_eq!(target(SpatialDirection::Right, &a), Some(b.clone()));
    assert_eq!(target(SpatialDirection::Down, &a), Some(c.clone()));
    assert_eq!(target(SpatialDirection::Left, &d), Some(c));
    assert_eq!(target(SpatialDirection::Up, &d), Some(b));
}

#[test]
fn equalizing_weights_splits_by_leaf_count() {
    let (tree, ..) = three_across();
    assert!(close(root_split(&tree).ratio, 0.5));
    assert!(close(root_split(&tree.equalized()).ratio, 1.0 / 3.0));
}

#[test]
fn equalizing_counts_other_orientations_as_one() {
    let (tree, _, b) = horizontal_split();
    let tree = tree.inserting(MockView::new(), &b, NewDirection::Down).unwrap();
    assert!(close(root_split(&tree.equalized()).ratio, 0.5));
}

#[test]
fn resizing_adjusts_the_ratio() {
    let cases = [
        (SpatialDirection::Right, NewDirection::Right, Size::new(1000.0, 500.0), 100, 0.6),
        (SpatialDirection::Left, NewDirection::Right, Size::new(1000.0, 500.0), 50, 0.45),
        (SpatialDirection::Down, NewDirection::Down, Size::new(500.0, 1000.0), 200, 0.7),
        (SpatialDirection::Up, NewDirection::Down, Size::new(500.0, 1000.0), 50, 0.45),
    ];
    for (resize, insert, bounds, pixels, expected) in cases {
        let (a, b) = (MockView::new(), MockView::new());
        let tree = Tree::with_view(a.clone()).inserting(b, &a, insert).unwrap();
        let resized = tree.resizing(&a.leaf(), pixels, resize, bounds).unwrap();
        assert!(close(root_split(&resized).ratio, expected), "{resize:?}");
    }
}

#[test]
fn resizing_clamps_the_ratio() {
    let (tree, a, _) = horizontal_split();
    let bounds = Size::new(1000.0, 500.0);
    let resized = tree.resizing(&a.leaf(), 5000, SpatialDirection::Right, bounds).unwrap();
    assert!(close(root_split(&resized).ratio, 0.9));
    let resized = tree.resizing(&a.leaf(), 5000, SpatialDirection::Left, bounds).unwrap();
    assert!(close(root_split(&resized).ratio, 0.1));
}

#[test]
fn resizing_needs_a_split_of_the_matching_orientation() {
    let (tree, a, _) = horizontal_split();
    let bounds = Size::new(1000.0, 500.0);
    assert_eq!(
        tree.resizing(&a.leaf(), 10, SpatialDirection::Down, bounds),
        Err(SplitError::ViewNotFound)
    );
    let missing = MockView::new();
    assert_eq!(
        tree.resizing(&missing.leaf(), 10, SpatialDirection::Right, bounds),
        Err(SplitError::ViewNotFound)
    );
}

#[test]
fn json_round_trip_preserves_the_tree() {
    let (tree, a, b) = horizontal_split();
    let json = serde_json::to_string(&tree).unwrap();
    let decoded: Tree = serde_json::from_str(&json).unwrap();
    assert!(decoded.find_by(|v| v.id == a.id).is_some());
    assert!(decoded.find_by(|v| v.id == b.id).is_some());
    assert!(decoded.is_split());
}

#[test]
fn json_matches_the_swift_layout() {
    let (tree, a, b) = horizontal_split();
    let value = serde_json::to_value(&tree).unwrap();
    let expected = serde_json::json!({
        "version": 1,
        "root": {"split": {
            "direction": {"horizontal": {}},
            "ratio": 0.5,
            "left": {"view": {"id": a.id}},
            "right": {"view": {"id": b.id}},
        }},
    });
    assert_eq!(value, expected);

    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    let value = serde_json::to_value(&zoomed).unwrap();
    assert_eq!(value["zoomed"], serde_json::json!({"path": [{"right": {}}]}));
}

#[test]
fn json_round_trip_preserves_the_zoomed_path() {
    let (tree, _, b) = horizontal_split();
    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    let json = serde_json::to_string(&zoomed).unwrap();
    let decoded: Tree = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.zoomed(), Some(&b.leaf()));
}

#[test]
fn json_rejects_unsupported_versions_and_bad_nodes() {
    assert!(serde_json::from_str::<Tree>(r#"{"version":2}"#).is_err());
    assert!(serde_json::from_str::<Tree>(r#"{"root":{"view":{"id":1}}}"#).is_err());
    assert!(serde_json::from_str::<Tree>(r#"{"version":1,"root":{}}"#).is_err());
    let two_cases = r#"{"version":1,"root":{"split":{"direction":{"horizontal":{},"vertical":{}},"ratio":0.5,
        "left":{"view":{"id":1}},"right":{"view":{"id":2}}}}}"#;
    assert!(serde_json::from_str::<Tree>(two_cases).is_err());
    let empty: Tree = serde_json::from_str(r#"{"version":1}"#).unwrap();
    assert!(empty.is_empty());
}

#[test]
fn a_single_view_cannot_be_zoomed() {
    let view = MockView::new();
    let tree = Tree::new(Some(view.leaf()), Some(view.leaf()));
    assert!(tree.zoomed().is_none());
    assert!(!tree.is_zoomed());
}

#[test]
fn the_zoomed_node_must_be_in_the_tree() {
    let (tree, _, _) = horizontal_split();
    let outside = MockView::new();
    let zoomed = Tree::new(tree.root().cloned(), Some(outside.leaf()));
    assert!(zoomed.zoomed().is_none());
    assert!(!zoomed.is_zoomed());
}

#[test]
fn removing_the_sibling_clears_the_zoom_when_no_longer_split() {
    let (tree, a, b) = horizontal_split();
    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    assert!(zoomed.is_zoomed());
    let remaining = zoomed.removing(&a.leaf());
    assert!(!remaining.is_split());
    assert!(remaining.zoomed().is_none());
}

#[test]
fn removing_the_zoomed_node_clears_the_zoom() {
    let (tree, _, b) = horizontal_split();
    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    let remaining = zoomed.removing(&b.leaf());
    assert!(remaining.zoomed().is_none());
    assert!(!remaining.is_zoomed());
}

#[test]
fn removing_a_sibling_keeps_the_zoom_while_still_split() {
    let (tree, a, b, c) = three_across();
    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    assert!(zoomed.is_zoomed());

    let remaining = zoomed.removing(&a.leaf());
    assert!(remaining.is_split());
    assert!(remaining.is_zoomed());

    let last = remaining.removing(&c.leaf());
    assert!(!last.is_split());
    assert!(last.zoomed().is_none());
}

#[test]
fn views_iterate_in_order() {
    let (tree, a, b, c) = three_across();
    assert_eq!(tree.len(), 3);
    assert_eq!(tree.get(0), Some(&a));
    assert_eq!(tree.get(1), Some(&b));
    assert_eq!(tree.get(2), Some(&c));
    assert_eq!(tree.get(3), None);
    assert_eq!(tree.views(), vec![&a, &b, &c]);

    let empty = Tree::default();
    assert_eq!(empty.len(), 0);
    assert!(empty.views().is_empty());
}

#[test]
fn structural_equality_ignores_ratios_but_not_views_or_shape() {
    let (tree, a, b) = horizontal_split();
    assert!(tree.is_structurally_equal(&tree));

    let resized = tree
        .resizing(&a.leaf(), 100, SpatialDirection::Right, Size::new(1000.0, 500.0))
        .unwrap();
    assert!(tree.is_structurally_equal(&resized));
    assert_ne!(tree, resized);

    let expanded = tree.inserting(MockView::new(), &b, NewDirection::Down).unwrap();
    assert!(!tree.is_structurally_equal(&expanded));

    let (other, _, _) = horizontal_split();
    assert!(!tree.is_structurally_equal(&other));
}

#[test]
fn structural_equality_of_nodes() {
    let (tree, ..) = horizontal_split();
    let split = root_split(&tree);
    assert!(split.left.is_structurally_equal(&split.left));
    assert!(!split.left.is_structurally_equal(&split.right));
}

#[test]
fn view_bounds_sums_and_maxes_by_orientation() {
    assert_eq!(Tree::default().view_bounds(size_of), Size::ZERO);

    let single = Tree::with_view(MockView::sized(500.0, 300.0));
    assert_eq!(single.view_bounds(size_of), Size::new(500.0, 300.0));

    let a = MockView::sized(400.0, 300.0);
    let horizontal = Tree::with_view(a.clone())
        .inserting(MockView::sized(200.0, 500.0), &a, NewDirection::Right)
        .unwrap();
    assert_eq!(horizontal.view_bounds(size_of), Size::new(600.0, 500.0));

    let a = MockView::sized(300.0, 200.0);
    let vertical = Tree::with_view(a.clone())
        .inserting(MockView::sized(500.0, 400.0), &a, NewDirection::Down)
        .unwrap();
    assert_eq!(vertical.view_bounds(size_of), Size::new(500.0, 600.0));
}

#[test]
fn node_lookup() {
    let a = MockView::new();
    let single = Tree::with_view(a.clone());
    assert_eq!(single.root().unwrap().node_for(&a), Some(&a.leaf()));
    assert_eq!(single.root().unwrap().node_for(&MockView::new()), None);

    let (tree, a, b) = horizontal_split();
    let root = tree.root().unwrap();
    assert_eq!(root.node_for(&a), Some(&a.leaf()));
    assert_eq!(root.node_for(&b), Some(&b.leaf()));
}

#[test]
fn path_round_trip() {
    let (tree, _, b, c) = three_across();
    let root = tree.root().unwrap();
    let path = root.path_to(&c.leaf()).unwrap();
    assert_eq!(path.path, vec![Component::Right, Component::Right]);
    assert_eq!(root.node_at(&path), Some(&c.leaf()));
    assert_eq!(root.node_at(&Path::default()), Some(root));
    assert!(root.path_to(&MockView::new().leaf()).is_none());
    assert!(root.node_at(&Path { path: vec![Component::Left, Component::Left] }).is_none());
    assert!(root.path_to(&b.leaf()).is_some());
}

#[test]
fn resizing_a_node_to_a_ratio() {
    let (tree, a, _) = horizontal_split();
    let resized = tree.root().unwrap().resizing(0.7);
    let Node::Split(split) = resized else { panic!("expected a split") };
    assert!(close(split.ratio, 0.7));

    let leaf = a.leaf();
    assert_eq!(leaf.resizing(0.7), leaf);
}

#[test]
fn does_border_edges() {
    let cases = [
        (SpatialDirection::Left, NewDirection::Right),
        (SpatialDirection::Right, NewDirection::Right),
        (SpatialDirection::Up, NewDirection::Down),
        (SpatialDirection::Down, NewDirection::Down),
    ];
    for (side, insert) in cases {
        let (a, b) = (MockView::new(), MockView::new());
        let tree = Tree::with_view(a.clone()).inserting(b.clone(), &a, insert).unwrap();
        let spatial = tree.root().unwrap().spatial(Some(Size::new(1000.0, 500.0)));
        let (border, other) = if matches!(side, SpatialDirection::Right | SpatialDirection::Down) {
            (&b, &a)
        } else {
            (&a, &b)
        };
        assert!(spatial.does_border(side, &border.leaf()), "{side:?}");
        assert!(!spatial.does_border(side, &other.leaf()), "{side:?}");
    }
}

#[test]
fn view_bounds_calculation() {
    let a = MockView::new();
    let single = Tree::with_view(a.clone());
    let bounds = Rect::new(0.0, 0.0, 1000.0, 500.0);
    assert_eq!(single.root().unwrap().calculate_view_bounds(bounds), vec![(a, bounds)]);

    let (tree, a, b) = horizontal_split();
    let result = tree.root().unwrap().calculate_view_bounds(bounds);
    assert_eq!(result[0], (a, Rect::new(0.0, 0.0, 500.0, 500.0)));
    assert_eq!(result[1], (b, Rect::new(500.0, 0.0, 500.0, 500.0)));

    let (a, b) = (MockView::new(), MockView::new());
    let vertical = Tree::with_view(a.clone()).inserting(b.clone(), &a, NewDirection::Down).unwrap();
    let result = vertical.root().unwrap().calculate_view_bounds(Rect::new(0.0, 0.0, 500.0, 1000.0));
    assert_eq!(result[0], (a, Rect::new(0.0, 500.0, 500.0, 500.0)));
    assert_eq!(result[1], (b, Rect::new(0.0, 0.0, 500.0, 500.0)));
}

#[test]
fn view_bounds_use_a_custom_ratio() {
    let (tree, a, b) = horizontal_split();
    let resized = tree.root().unwrap().resizing(0.3);
    let result = resized.calculate_view_bounds(Rect::new(0.0, 0.0, 1000.0, 400.0));
    let find = |view: &MockView| result.iter().find(|(v, _)| v == view).unwrap().1;
    assert_eq!(find(&a).width, 300.0);
    assert_eq!(find(&b).width, 700.0);
    assert_eq!(find(&b).min_x(), 300.0);
}

#[test]
fn view_bounds_of_a_grid() {
    let (tree, [a, b, c, d]) = grid();
    let result = tree.root().unwrap().calculate_view_bounds(Rect::new(0.0, 0.0, 1000.0, 800.0));
    assert_eq!(result.len(), 4);
    let find = |view: &MockView| result.iter().find(|(v, _)| v == view).unwrap().1;
    assert_eq!(find(&a), Rect::new(0.0, 400.0, 500.0, 400.0));
    assert_eq!(find(&b), Rect::new(500.0, 400.0, 500.0, 400.0));
    assert_eq!(find(&c), Rect::new(0.0, 0.0, 500.0, 400.0));
    assert_eq!(find(&d), Rect::new(500.0, 0.0, 500.0, 400.0));
}

#[test]
fn slots_in_each_direction() {
    let cases = [
        (SpatialDirection::Right, NewDirection::Right),
        (SpatialDirection::Left, NewDirection::Right),
        (SpatialDirection::Down, NewDirection::Down),
        (SpatialDirection::Up, NewDirection::Down),
    ];
    for (direction, insert) in cases {
        let (a, b) = (MockView::new(), MockView::new());
        let tree = Tree::with_view(a.clone()).inserting(b.clone(), &a, insert).unwrap();
        let spatial = tree.root().unwrap().spatial(Some(Size::new(1000.0, 500.0)));
        let (from, expected) = if matches!(direction, SpatialDirection::Right | SpatialDirection::Down) {
            (&a, &b)
        } else {
            (&b, &a)
        };
        let slots = spatial.slots_in(direction, &from.leaf());
        assert_eq!(slots.len(), 1, "{direction:?}");
        assert_eq!(slots[0].node, expected.leaf());
    }
}

#[test]
fn slots_in_a_grid() {
    let (tree, [a, b, c, d]) = grid();
    let spatial = tree.root().unwrap().spatial(Some(Size::new(1000.0, 800.0)));
    let nodes = |direction, from: &MockView| -> Vec<Node<MockView>> {
        spatial.slots_in(direction, &from.leaf()).into_iter().map(|s| s.node.clone()).collect()
    };
    assert!(nodes(SpatialDirection::Right, &a).contains(&b.leaf()));
    assert!(nodes(SpatialDirection::Down, &a).contains(&c.leaf()));
    assert!(nodes(SpatialDirection::Left, &d).contains(&c.leaf()));
    assert!(nodes(SpatialDirection::Up, &d).contains(&b.leaf()));
}

#[test]
fn slots_are_empty_when_nothing_lies_in_the_direction() {
    let (tree, a, b) = horizontal_split();
    let spatial = tree.root().unwrap().spatial(Some(Size::new(1000.0, 500.0)));
    assert!(spatial.slots_in(SpatialDirection::Left, &a.leaf()).is_empty());
    assert!(spatial.slots_in(SpatialDirection::Right, &b.leaf()).is_empty());
    assert!(spatial.slots_in(SpatialDirection::Up, &a.leaf()).is_empty());
    assert!(spatial.slots_in(SpatialDirection::Down, &b.leaf()).is_empty());
}

#[test]
fn spatial_without_bounds_uses_a_grid_of_cells() {
    let (tree, ..) = grid();
    let spatial = tree.root().unwrap().spatial(None);
    assert_eq!(spatial.slots[0].bounds, Rect::new(0.0, 0.0, 2.0, 2.0));
}

#[test]
fn mapping_views_keeps_shape_and_zoom() {
    let (tree, _, b, _) = three_across();
    let zoomed = Tree::new(tree.root().cloned(), Some(b.leaf()));
    let mapped = zoomed.map(|v| v.id * 10);
    assert_eq!(mapped.views(), vec![&(zoomed.views()[0].id * 10), &(b.id * 10), &(zoomed.views()[2].id * 10)]);
    assert_eq!(mapped.zoomed(), Some(&Node::Leaf(b.id * 10)));
}
