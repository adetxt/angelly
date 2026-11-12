//! A tree of views that can be divided, laid out left/right or top/bottom.
//!
//! Views are compared with `PartialEq`; use a handle type whose equality is
//! identity (an id, an `Rc` pointer wrapper) rather than structural equality.

use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::geometry::{Rect, Size};
use super::swift::swift_enum;

swift_enum! {
    /// How a split lays out its children: `Horizontal` puts them left and right,
    /// `Vertical` top and bottom.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Direction {
        Horizontal => "horizontal",
        Vertical => "vertical",
    }
}

swift_enum! {
    /// One step of a [`Path`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum Component {
        Left => "left",
        Right => "right",
    }
}

/// The path from the root to a node.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Path {
    pub path: Vec<Component>,
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }
}

/// Where a new view goes relative to the view it is inserted next to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewDirection {
    Left,
    Right,
    Down,
    Up,
}

/// Direction of spatial navigation and resizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialDirection {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Previous,
    Next,
    Spatial(SpatialDirection),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitError {
    ViewNotFound,
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("view not found in split tree")
    }
}

impl std::error::Error for SplitError {}

/// A node is either a leaf (a view) or a split with two children.
#[derive(Debug, Clone, PartialEq)]
pub enum Node<V> {
    Leaf(V),
    Split(Split<V>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(bound(serialize = "V: Serialize", deserialize = "V: Deserialize<'de>"))]
pub struct Split<V> {
    pub direction: Direction,
    pub ratio: f64,
    pub left: Box<Node<V>>,
    pub right: Box<Node<V>>,
}

impl<V> Split<V> {
    fn with_ratio(&self, ratio: f64) -> Split<V>
    where
        V: Clone,
    {
        Split {
            direction: self.direction,
            ratio,
            left: self.left.clone(),
            right: self.right.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slot<V> {
    pub node: Node<V>,
    /// Bounds relative to the other slots, not physical pixels.
    pub bounds: Rect,
}

/// The tree mapped to 2D space so navigation can follow the physical layout.
/// The origin is the top-left corner and Y grows downward.
#[derive(Debug, Clone, PartialEq)]
pub struct Spatial<V> {
    pub slots: Vec<Slot<V>>,
}

impl<V: Clone + PartialEq> Spatial<V> {
    /// Slots in `direction` from `reference`, closest top-left corner first.
    ///
    /// Splits and leaves are both returned; callers that want a focusable view
    /// should prefer the first leaf.
    pub fn slots_in(&self, direction: SpatialDirection, reference: &Node<V>) -> Vec<&Slot<V>> {
        let Some(origin) = self.slots.iter().find(|s| &s.node == reference) else {
            return Vec::new();
        };
        let origin = origin.bounds;
        let distance = |bounds: &Rect| {
            let dx = bounds.min_x() - origin.min_x();
            let dy = bounds.min_y() - origin.min_y();
            (dx * dx + dy * dy).sqrt()
        };

        let mut result: Vec<&Slot<V>> = self
            .slots
            .iter()
            .filter(|slot| {
                slot.node != *reference
                    && match direction {
                        SpatialDirection::Left => slot.bounds.max_x() <= origin.min_x(),
                        SpatialDirection::Right => slot.bounds.min_x() >= origin.max_x(),
                        SpatialDirection::Up => slot.bounds.max_y() <= origin.min_y(),
                        SpatialDirection::Down => slot.bounds.min_y() >= origin.max_y(),
                    }
            })
            .collect();
        result.sort_by(|a, b| distance(&a.bounds).total_cmp(&distance(&b.bounds)));
        result
    }

    /// Whether `node` touches the given outer edge of the whole layout.
    pub fn does_border(&self, side: SpatialDirection, node: &Node<V>) -> bool {
        let Some(slot) = self.slots.iter().find(|s| &s.node == node) else {
            return false;
        };
        let bounds = slot.bounds;
        let edge = |pick: fn(&Rect) -> f64, fold: fn(f64, f64) -> f64| {
            self.slots.iter().map(|s| pick(&s.bounds)).reduce(fold).unwrap_or(0.0)
        };
        match side {
            SpatialDirection::Up => bounds.min_y() == edge(Rect::min_y, f64::min),
            SpatialDirection::Down => bounds.max_y() == edge(Rect::max_y, f64::max),
            SpatialDirection::Left => bounds.min_x() == edge(Rect::min_x, f64::min),
            SpatialDirection::Right => bounds.max_x() == edge(Rect::max_x, f64::max),
        }
    }
}

impl<V> Node<V> {
    /// All leaf views from left to right.
    pub fn leaves(&self) -> Vec<&V> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, out: &mut Vec<&'a V>) {
        match self {
            Node::Leaf(view) => out.push(view),
            Node::Split(split) => {
                split.left.collect_leaves(out);
                split.right.collect_leaves(out);
            }
        }
    }

    pub fn leftmost_leaf(&self) -> &V {
        match self {
            Node::Leaf(view) => view,
            Node::Split(split) => split.left.leftmost_leaf(),
        }
    }

    pub fn rightmost_leaf(&self) -> &V {
        match self {
            Node::Leaf(view) => view,
            Node::Split(split) => split.right.rightmost_leaf(),
        }
    }

    /// Same tree with every view replaced by `f(view)`.
    pub fn map<U>(&self, f: &mut impl FnMut(&V) -> U) -> Node<U> {
        match self {
            Node::Leaf(view) => Node::Leaf(f(view)),
            Node::Split(split) => Node::Split(Split {
                direction: split.direction,
                ratio: split.ratio,
                left: Box::new(split.left.map(f)),
                right: Box::new(split.right.map(f)),
            }),
        }
    }

    /// Grid size in columns and rows, one cell per leaf.
    fn dimensions(&self) -> (u64, u64) {
        match self {
            Node::Leaf(_) => (1, 1),
            Node::Split(split) => {
                let (lw, lh) = split.left.dimensions();
                let (rw, rh) = split.right.dimensions();
                match split.direction {
                    Direction::Horizontal => (lw + rw, lh.max(rh)),
                    Direction::Vertical => (lw.max(rw), lh + rh),
                }
            }
        }
    }

    /// Total size of the laid out views, ignoring offsets and padding.
    pub fn view_bounds(&self, size_of: &impl Fn(&V) -> Size) -> Size {
        match self {
            Node::Leaf(view) => size_of(view),
            Node::Split(split) => {
                let left = split.left.view_bounds(size_of);
                let right = split.right.view_bounds(size_of);
                match split.direction {
                    Direction::Horizontal => {
                        Size::new(left.width + right.width, left.height.max(right.height))
                    }
                    Direction::Vertical => {
                        Size::new(left.width.max(right.width), left.height + right.height)
                    }
                }
            }
        }
    }

    fn weight_for(&self, direction: Direction) -> u64 {
        match self {
            Node::Leaf(_) => 1,
            Node::Split(split) if split.direction == direction => {
                split.left.weight_for(direction) + split.right.weight_for(direction)
            }
            Node::Split(_) => 1,
        }
    }
}

impl<V: Clone + PartialEq> Node<V> {
    pub fn find_by(&self, predicate: &impl Fn(&V) -> bool) -> Option<&Node<V>> {
        match self {
            Node::Leaf(view) => predicate(view).then_some(self),
            Node::Split(split) => split
                .left
                .find_by(predicate)
                .or_else(|| split.right.find_by(predicate)),
        }
    }

    /// The leaf node holding `view`.
    pub fn node_for(&self, view: &V) -> Option<&Node<V>> {
        self.find_by(&|candidate| candidate == view)
    }

    /// The path to `target`, or `None` when it is not in the tree.
    pub fn path_to(&self, target: &Node<V>) -> Option<Path> {
        fn search<V: PartialEq>(
            current: &Node<V>,
            target: &Node<V>,
            components: &mut Vec<Component>,
        ) -> bool {
            if current == target {
                return true;
            }
            let Node::Split(split) = current else {
                return false;
            };
            for (component, child) in [(Component::Left, &split.left), (Component::Right, &split.right)] {
                components.push(component);
                if search(child, target, components) {
                    return true;
                }
                components.pop();
            }
            false
        }

        let mut components = Vec::new();
        search(self, target, &mut components).then_some(Path { path: components })
    }

    pub fn node_at(&self, path: &Path) -> Option<&Node<V>> {
        self.node_at_components(&path.path)
    }

    fn node_at_components(&self, path: &[Component]) -> Option<&Node<V>> {
        let Some((first, rest)) = path.split_first() else {
            return Some(self);
        };
        let Node::Split(split) = self else {
            return None;
        };
        match first {
            Component::Left => split.left.node_at_components(rest),
            Component::Right => split.right.node_at_components(rest),
        }
    }

    /// Splits the leaf holding `at`, placing `view` on the side given by `direction`.
    pub fn inserting(&self, view: V, at: &V, direction: NewDirection) -> Result<Node<V>, SplitError> {
        let path = self
            .path_to(&Node::Leaf(at.clone()))
            .ok_or(SplitError::ViewNotFound)?;
        let (split_direction, new_first) = match direction {
            NewDirection::Left => (Direction::Horizontal, true),
            NewDirection::Right => (Direction::Horizontal, false),
            NewDirection::Up => (Direction::Vertical, true),
            NewDirection::Down => (Direction::Vertical, false),
        };
        let new = Box::new(Node::Leaf(view));
        let existing = Box::new(Node::Leaf(at.clone()));
        let (left, right) = if new_first { (new, existing) } else { (existing, new) };
        self.replacing_at(
            &path.path,
            &Node::Split(Split {
                direction: split_direction,
                ratio: 0.5,
                left,
                right,
            }),
        )
    }

    pub(crate) fn replacing_at(&self, path: &[Component], new: &Node<V>) -> Result<Node<V>, SplitError> {
        let Some((first, rest)) = path.split_first() else {
            return Ok(new.clone());
        };
        let Node::Split(split) = self else {
            return Err(SplitError::ViewNotFound);
        };
        let (left, right) = match first {
            Component::Left => (
                Box::new(split.left.replacing_at(rest, new)?),
                split.right.clone(),
            ),
            Component::Right => (
                split.left.clone(),
                Box::new(split.right.replacing_at(rest, new)?),
            ),
        };
        Ok(Node::Split(Split {
            direction: split.direction,
            ratio: split.ratio,
            left,
            right,
        }))
    }

    /// Removes `target`; its sibling takes the parent's place. `None` when nothing remains.
    pub fn remove(&self, target: &Node<V>) -> Option<Node<V>> {
        if self == target {
            return None;
        }
        let Node::Split(split) = self else {
            return Some(self.clone());
        };
        match (split.left.remove(target), split.right.remove(target)) {
            (None, None) => None,
            (None, Some(right)) => Some(right),
            (Some(left), None) => Some(left),
            (Some(left), Some(right)) => Some(Node::Split(Split {
                direction: split.direction,
                ratio: split.ratio,
                left: Box::new(left),
                right: Box::new(right),
            })),
        }
    }

    /// A split with its ratio set to `ratio`; leaves are returned unchanged.
    pub fn resizing(&self, ratio: f64) -> Node<V> {
        match self {
            Node::Leaf(_) => self.clone(),
            Node::Split(split) => Node::Split(split.with_ratio(ratio)),
        }
    }

    /// Sets every split's ratio from the number of leaves on each side.
    pub fn equalize(&self) -> Node<V> {
        let Node::Split(split) = self else {
            return self.clone();
        };
        let left_weight = split.left.weight_for(split.direction);
        let right_weight = split.right.weight_for(split.direction);
        Node::Split(Split {
            direction: split.direction,
            ratio: left_weight as f64 / (left_weight + right_weight) as f64,
            left: Box::new(split.left.equalize()),
            right: Box::new(split.right.equalize()),
        })
    }

    /// Bounds of every view inside `bounds`. Y grows upward, so the first child of a
    /// vertical split is the upper one.
    pub fn calculate_view_bounds(&self, bounds: Rect) -> Vec<(V, Rect)> {
        match self {
            Node::Leaf(view) => vec![(view.clone(), bounds)],
            Node::Split(split) => {
                let (left, right) = match split.direction {
                    Direction::Horizontal => (
                        Rect::new(bounds.x, bounds.y, bounds.width * split.ratio, bounds.height),
                        Rect::new(
                            bounds.min_x() + bounds.width * split.ratio,
                            bounds.y,
                            bounds.width * (1.0 - split.ratio),
                            bounds.height,
                        ),
                    ),
                    Direction::Vertical => (
                        Rect::new(
                            bounds.x,
                            bounds.min_y() + bounds.height * split.ratio,
                            bounds.width,
                            bounds.height * (1.0 - split.ratio),
                        ),
                        Rect::new(bounds.x, bounds.y, bounds.width, bounds.height * split.ratio),
                    ),
                };
                let mut result = split.left.calculate_view_bounds(left);
                result.extend(split.right.calculate_view_bounds(right));
                result
            }
        }
    }

    /// The spatial layout, within `bounds` or, when absent, a grid with one cell per leaf.
    pub fn spatial(&self, bounds: Option<Size>) -> Spatial<V> {
        let size = bounds.unwrap_or_else(|| {
            let (columns, rows) = self.dimensions();
            Size::new(columns as f64, rows as f64)
        });
        let mut slots = Vec::new();
        self.collect_slots(Rect::new(0.0, 0.0, size.width, size.height), &mut slots);
        Spatial { slots }
    }

    fn collect_slots(&self, bounds: Rect, slots: &mut Vec<Slot<V>>) {
        slots.push(Slot {
            node: self.clone(),
            bounds,
        });
        let Node::Split(split) = self else {
            return;
        };
        let (left, right) = match split.direction {
            Direction::Horizontal => (
                Rect::new(bounds.x, bounds.y, bounds.width * split.ratio, bounds.height),
                Rect::new(
                    bounds.min_x() + bounds.width * split.ratio,
                    bounds.y,
                    bounds.width * (1.0 - split.ratio),
                    bounds.height,
                ),
            ),
            Direction::Vertical => (
                Rect::new(bounds.x, bounds.y, bounds.width, bounds.height * split.ratio),
                Rect::new(
                    bounds.x,
                    bounds.min_y() + bounds.height * split.ratio,
                    bounds.width,
                    bounds.height * (1.0 - split.ratio),
                ),
            ),
        };
        split.left.collect_slots(left, slots);
        split.right.collect_slots(right, slots);
    }

    /// Whether two nodes have the same shape and the same views in the same
    /// places. Ratios are ignored because they change while resizing.
    pub fn is_structurally_equal(&self, other: &Node<V>) -> bool {
        match (self, other) {
            (Node::Leaf(a), Node::Leaf(b)) => a == b,
            (Node::Split(a), Node::Split(b)) => {
                a.direction == b.direction
                    && a.left.is_structurally_equal(&b.left)
                    && a.right.is_structurally_equal(&b.right)
            }
            _ => false,
        }
    }
}

impl<V: Serialize> Serialize for Node<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Node::Leaf(view) => map.serialize_entry("view", view)?,
            Node::Split(split) => map.serialize_entry("split", split)?,
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Node<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "V: Deserialize<'de>"))]
        struct Raw<V> {
            view: Option<V>,
            split: Option<Split<V>>,
        }

        let raw = Raw::<V>::deserialize(deserializer)?;
        match (raw.view, raw.split) {
            (Some(view), _) => Ok(Node::Leaf(view)),
            (None, Some(split)) => Ok(Node::Split(split)),
            (None, None) => Err(serde::de::Error::custom("No valid node type found")),
        }
    }
}

const CURRENT_VERSION: i64 = 1;

/// A tree of views with an optional zoomed node.
#[derive(Debug, Clone, PartialEq)]
pub struct SplitTree<V> {
    root: Option<Node<V>>,
    zoomed: Option<Node<V>>,
}

impl<V> Default for SplitTree<V> {
    fn default() -> Self {
        Self { root: None, zoomed: None }
    }
}

impl<V: Clone + PartialEq> SplitTree<V> {
    /// A tree can only be zoomed when it is split and the zoomed node is inside it.
    pub fn new(root: Option<Node<V>>, zoomed: Option<Node<V>>) -> Self {
        let zoomed = match (&root, zoomed) {
            (Some(root @ Node::Split(_)), Some(zoomed)) if root.path_to(&zoomed).is_some() => Some(zoomed),
            _ => None,
        };
        Self { root, zoomed }
    }

    pub fn with_view(view: V) -> Self {
        Self::new(Some(Node::Leaf(view)), None)
    }

    pub fn root(&self) -> Option<&Node<V>> {
        self.root.as_ref()
    }

    pub fn zoomed(&self) -> Option<&Node<V>> {
        self.zoomed.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn is_split(&self) -> bool {
        matches!(self.root, Some(Node::Split(_)))
    }

    pub fn is_zoomed(&self) -> bool {
        self.is_split() && self.zoomed.is_some()
    }

    pub fn contains(&self, node: &Node<V>) -> bool {
        self.root.as_ref().is_some_and(|root| root.path_to(node).is_some())
    }

    /// Leaf views from left to right.
    pub fn views(&self) -> Vec<&V> {
        self.root.as_ref().map(Node::leaves).unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.views().len()
    }

    pub fn get(&self, index: usize) -> Option<&V> {
        self.views().get(index).copied()
    }

    pub fn find_by(&self, predicate: impl Fn(&V) -> bool) -> Option<&Node<V>> {
        self.root.as_ref()?.find_by(&predicate)
    }

    /// Splits the leaf holding `at` to add `view`. Always clears the zoom.
    pub fn inserting(&self, view: V, at: &V, direction: NewDirection) -> Result<Self, SplitError> {
        let root = self.root.as_ref().ok_or(SplitError::ViewNotFound)?;
        Ok(Self::new(Some(root.inserting(view, at, direction)?), None))
    }

    /// Removes a node, promoting its sibling. Clears the zoom if the zoomed node went away.
    pub fn removing(&self, target: &Node<V>) -> Self {
        let Some(root) = &self.root else {
            return self.clone();
        };
        if root == target {
            return Self::default();
        }
        let zoomed = self.zoomed.clone().filter(|zoomed| zoomed != target);
        Self::new(root.remove(target), zoomed)
    }

    pub fn replacing(&self, node: &Node<V>, with: Node<V>) -> Result<Self, SplitError> {
        let root = self.root.as_ref().ok_or(SplitError::ViewNotFound)?;
        let path = root.path_to(node).ok_or(SplitError::ViewNotFound)?;
        let new_root = root.replacing_at(&path.path, &with)?;
        let zoomed = if self.zoomed.as_ref() == Some(node) {
            Some(with)
        } else {
            self.zoomed.clone()
        };
        Ok(Self::new(Some(new_root), zoomed))
    }

    /// The view that should receive focus when moving from `current`.
    pub fn focus_target(&self, direction: FocusDirection, current: &Node<V>) -> Option<V> {
        let root = self.root.as_ref()?;
        match direction {
            FocusDirection::Previous => {
                let leaves = root.leaves();
                let index = leaves.iter().position(|v| *v == current.leftmost_leaf())?;
                Some(leaves[(index + leaves.len() - 1) % leaves.len()].clone())
            }
            FocusDirection::Next => {
                let leaves = root.leaves();
                let index = leaves.iter().position(|v| *v == current.rightmost_leaf())?;
                Some(leaves[(index + 1) % leaves.len()].clone())
            }
            FocusDirection::Spatial(spatial_direction) => {
                let spatial = root.spatial(None);
                let candidates = spatial.slots_in(spatial_direction, current);
                let best = candidates
                    .iter()
                    .find(|slot| matches!(slot.node, Node::Leaf(_)))
                    .or_else(|| candidates.first())?;
                Some(match (&best.node, spatial_direction) {
                    (Node::Leaf(view), _) => view.clone(),
                    (node, SpatialDirection::Up | SpatialDirection::Left) => node.leftmost_leaf().clone(),
                    (node, SpatialDirection::Down | SpatialDirection::Right) => node.rightmost_leaf().clone(),
                })
            }
        }
    }

    /// Every split's ratio set from the number of leaves on each side.
    pub fn equalized(&self) -> Self {
        match &self.root {
            Some(root) => Self::new(Some(root.equalize()), self.zoomed.clone()),
            None => self.clone(),
        }
    }

    /// Moves the nearest enclosing split of the matching orientation by `pixels`
    /// within a layout of size `bounds`. Always clears the zoom.
    pub fn resizing(
        &self,
        node: &Node<V>,
        pixels: u16,
        direction: SpatialDirection,
        bounds: Size,
    ) -> Result<Self, SplitError> {
        let root = self.root.as_ref().ok_or(SplitError::ViewNotFound)?;
        let path = root.path_to(node).ok_or(SplitError::ViewNotFound)?;
        let target = match direction {
            SpatialDirection::Up | SpatialDirection::Down => Direction::Vertical,
            SpatialDirection::Left | SpatialDirection::Right => Direction::Horizontal,
        };

        let (split_path, split_node, split) = (0..path.path.len())
            .rev()
            .find_map(|depth| {
                let parent = Path { path: path.path[..depth].to_vec() };
                match root.node_at(&parent) {
                    Some(found @ Node::Split(split)) if split.direction == target => {
                        Some((parent, found, split))
                    }
                    _ => None,
                }
            })
            .ok_or(SplitError::ViewNotFound)?;

        let spatial = root.spatial(Some(bounds));
        let slot = spatial
            .slots
            .iter()
            .find(|slot| &slot.node == split_node)
            .ok_or(SplitError::ViewNotFound)?;

        let offset = f64::from(pixels);
        let ratio = match (split.direction, direction) {
            (Direction::Horizontal, SpatialDirection::Left) => split.ratio - offset / slot.bounds.width,
            (Direction::Horizontal, SpatialDirection::Right) => split.ratio + offset / slot.bounds.width,
            (Direction::Vertical, SpatialDirection::Up) => split.ratio - offset / slot.bounds.height,
            (Direction::Vertical, SpatialDirection::Down) => split.ratio + offset / slot.bounds.height,
            _ => return Err(SplitError::ViewNotFound),
        }
        .clamp(0.1, 0.9);

        let new_root = root.replacing_at(&split_path.path, &Node::Split(split.with_ratio(ratio)))?;
        Ok(Self::new(Some(new_root), None))
    }

    /// Total size of the laid out views.
    pub fn view_bounds(&self, size_of: impl Fn(&V) -> Size) -> Size {
        self.root
            .as_ref()
            .map_or(Size::ZERO, |root| root.view_bounds(&size_of))
    }

    /// Same tree with every view replaced by `f(view)`. The zoomed node follows its path.
    pub fn map<U: Clone + PartialEq>(&self, mut f: impl FnMut(&V) -> U) -> SplitTree<U> {
        let Some(root) = &self.root else {
            return SplitTree::default();
        };
        let mapped = root.map(&mut f);
        let zoomed = self
            .zoomed
            .as_ref()
            .and_then(|zoomed| root.path_to(zoomed))
            .and_then(|path| mapped.node_at(&path).cloned());
        SplitTree::new(Some(mapped), zoomed)
    }

    /// Whether the trees have the same shape and views, ignoring ratios.
    pub fn is_structurally_equal(&self, other: &Self) -> bool {
        fn same<V: Clone + PartialEq>(a: &Option<Node<V>>, b: &Option<Node<V>>) -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => a.is_structurally_equal(b),
                _ => false,
            }
        }
        same(&self.root, &other.root) && same(&self.zoomed, &other.zoomed)
    }
}

impl<V: Clone + PartialEq + Serialize> Serialize for SplitTree<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let path = self
            .zoomed
            .as_ref()
            .and_then(|zoomed| self.root.as_ref()?.path_to(zoomed));
        let fields = 1 + usize::from(self.root.is_some()) + usize::from(path.is_some());
        let mut state = serializer.serialize_struct("SplitTree", fields)?;
        state.serialize_field("version", &CURRENT_VERSION)?;
        if let Some(root) = &self.root {
            state.serialize_field("root", root)?;
        }
        if let Some(path) = &path {
            state.serialize_field("zoomed", path)?;
        }
        state.end()
    }
}

impl<'de, V: Clone + PartialEq + Deserialize<'de>> Deserialize<'de> for SplitTree<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "V: Deserialize<'de>"))]
        struct Raw<V> {
            version: i64,
            root: Option<Node<V>>,
            zoomed: Option<Path>,
        }

        let raw = Raw::<V>::deserialize(deserializer)?;
        if raw.version != CURRENT_VERSION {
            return Err(serde::de::Error::custom(format!(
                "Unsupported SplitTree version: {}",
                raw.version
            )));
        }
        let zoomed = match (&raw.root, &raw.zoomed) {
            (Some(root @ Node::Split(_)), Some(path)) => root.node_at(path).cloned(),
            _ => None,
        };
        Ok(Self { root: raw.root, zoomed })
    }
}

#[cfg(test)]
mod tests;
