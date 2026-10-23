//! In-window split-pane layout for a workspace.
//!
//! A [`PaneNode`] is a binary tree whose leaves are the tabs currently shown
//! side-by-side (tmux-style). A workspace with no split shows a single tab and
//! stores `None` for its tree (see [`crate::workspaces::workspace::Workspace::pane_tree`]),
//! so existing state and the common case are unaffected.
//!
//! All operations are pure: they return a new tree rather than mutating.
//!
//! The split ratio is stored as an integer permille (0..=[`RATIO_SCALE`]) of the
//! space given to the first child.

use crate::ids::TabId;
use serde::{Deserialize, Serialize};

/// Full-scale value for a split ratio (permille). A ratio of `RATIO_SCALE / 2`
/// is an even split.
pub const RATIO_SCALE: u16 = 1000;
/// Default ratio for a fresh split: even.
pub const DEFAULT_RATIO: u16 = RATIO_SCALE / 2;
/// Smallest share either side of a split may take, so a pane can't be dragged
/// to zero width/height.
pub const MIN_RATIO: u16 = 50;

/// Direction in which a split divides its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    /// Children laid out left | right (a vertical divider).
    Horizontal,
    /// Children laid out top / bottom (a horizontal divider).
    Vertical,
}

/// One step from a split toward a descendant: into the first or second child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneBranch {
    First,
    Second,
}

/// A node in a workspace's pane layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneNode {
    /// A single tab filling its region.
    Leaf { tab: TabId },
    /// Two child regions divided by `ratio` along `dir`.
    Split {
        dir: SplitDir,
        /// Permille of the space allotted to `first` (see [`RATIO_SCALE`]).
        ratio: u16,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    /// A leaf pane showing `tab`.
    pub fn leaf(tab: TabId) -> Self {
        PaneNode::Leaf { tab }
    }

    /// True if `self` is a single leaf (no split).
    pub fn is_leaf(&self) -> bool {
        matches!(self, PaneNode::Leaf { .. })
    }

    /// Every tab shown by this tree, left-to-right / top-to-bottom.
    pub fn leaves(&self) -> Vec<TabId> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<TabId>) {
        match self {
            PaneNode::Leaf { tab } => out.push(*tab),
            PaneNode::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
    }

    /// Whether `tab` is shown anywhere in this tree.
    pub fn contains(&self, tab: TabId) -> bool {
        match self {
            PaneNode::Leaf { tab: t } => *t == tab,
            PaneNode::Split { first, second, .. } => first.contains(tab) || second.contains(tab),
        }
    }

    /// The first (top-left) leaf's tab. A tree always has at least one leaf.
    pub fn first_leaf(&self) -> TabId {
        match self {
            PaneNode::Leaf { tab } => *tab,
            PaneNode::Split { first, .. } => first.first_leaf(),
        }
    }

    /// Replace the leaf showing `target` with a split: `target` and `new_tab`
    /// side-by-side along `dir`. `place_new_first` puts the new tab in the
    /// first (left/top) slot. No-op if `target` isn't a leaf here.
    pub fn with_split(
        &self,
        target: TabId,
        dir: SplitDir,
        new_tab: TabId,
        place_new_first: bool,
    ) -> Self {
        match self {
            PaneNode::Leaf { tab } if *tab == target => {
                let existing = Box::new(PaneNode::leaf(*tab));
                let added = Box::new(PaneNode::leaf(new_tab));
                let (first, second) = if place_new_first {
                    (added, existing)
                } else {
                    (existing, added)
                };
                PaneNode::Split {
                    dir,
                    ratio: DEFAULT_RATIO,
                    first,
                    second,
                }
            }
            PaneNode::Leaf { .. } => self.clone(),
            PaneNode::Split {
                dir: d,
                ratio,
                first,
                second,
            } => PaneNode::Split {
                dir: *d,
                ratio: *ratio,
                first: Box::new(first.with_split(target, dir, new_tab, place_new_first)),
                second: Box::new(second.with_split(target, dir, new_tab, place_new_first)),
            },
        }
    }

    /// Remove the leaf showing `target`, collapsing its parent split so the
    /// sibling takes the freed space. Returns `None` if `target` was the only
    /// leaf (the tree becomes empty).
    pub fn with_closed_leaf(&self, target: TabId) -> Option<Self> {
        match self {
            PaneNode::Leaf { tab } if *tab == target => None,
            PaneNode::Leaf { .. } => Some(self.clone()),
            PaneNode::Split {
                dir,
                ratio,
                first,
                second,
            } => {
                let new_first = first.with_closed_leaf(target);
                let new_second = second.with_closed_leaf(target);
                match (new_first, new_second) {
                    (Some(f), Some(s)) => Some(PaneNode::Split {
                        dir: *dir,
                        ratio: *ratio,
                        first: Box::new(f),
                        second: Box::new(s),
                    }),
                    // One side collapsed entirely: promote the surviving side.
                    (None, Some(s)) => Some(s),
                    (Some(f), None) => Some(f),
                    (None, None) => None,
                }
            }
        }
    }

    /// Swap the tab shown by the leaf `old` for `new`, keeping the layout.
    /// No-op if `old` isn't a leaf here.
    pub fn with_leaf_replaced(&self, old: TabId, new: TabId) -> Self {
        match self {
            PaneNode::Leaf { tab } if *tab == old => PaneNode::leaf(new),
            PaneNode::Leaf { .. } => self.clone(),
            PaneNode::Split {
                dir,
                ratio,
                first,
                second,
            } => PaneNode::Split {
                dir: *dir,
                ratio: *ratio,
                first: Box::new(first.with_leaf_replaced(old, new)),
                second: Box::new(second.with_leaf_replaced(old, new)),
            },
        }
    }

    /// Set the ratio of the split reached by following `path` from the root.
    /// The ratio is clamped to [`MIN_RATIO`, `RATIO_SCALE - MIN_RATIO`]. An
    /// empty path targets the root; a path that doesn't land on a split is a
    /// no-op.
    pub fn with_ratio_at(&self, path: &[PaneBranch], ratio: u16) -> Self {
        let clamped = ratio.clamp(MIN_RATIO, RATIO_SCALE - MIN_RATIO);
        match self {
            PaneNode::Leaf { .. } => self.clone(),
            PaneNode::Split {
                dir,
                ratio: cur,
                first,
                second,
            } => match path.split_first() {
                None => PaneNode::Split {
                    dir: *dir,
                    ratio: clamped,
                    first: first.clone(),
                    second: second.clone(),
                },
                Some((PaneBranch::First, rest)) => PaneNode::Split {
                    dir: *dir,
                    ratio: *cur,
                    first: Box::new(first.with_ratio_at(rest, ratio)),
                    second: second.clone(),
                },
                Some((PaneBranch::Second, rest)) => PaneNode::Split {
                    dir: *dir,
                    ratio: *cur,
                    first: first.clone(),
                    second: Box::new(second.with_ratio_at(rest, ratio)),
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab() -> TabId {
        TabId::new()
    }

    #[test]
    fn leaf_basics() {
        let a = tab();
        let node = PaneNode::leaf(a);
        assert!(node.is_leaf());
        assert_eq!(node.leaves(), vec![a]);
        assert_eq!(node.first_leaf(), a);
        assert!(node.contains(a));
        assert!(!node.contains(tab()));
    }

    #[test]
    fn split_adds_a_leaf_in_order() {
        let a = tab();
        let b = tab();
        let node = PaneNode::leaf(a).with_split(a, SplitDir::Horizontal, b, false);
        assert_eq!(node.leaves(), vec![a, b]); // existing first, new second
        assert!(!node.is_leaf());

        let node_first = PaneNode::leaf(a).with_split(a, SplitDir::Horizontal, b, true);
        assert_eq!(node_first.leaves(), vec![b, a]); // new placed first
    }

    #[test]
    fn nested_split_targets_the_right_leaf() {
        let a = tab();
        let b = tab();
        let c = tab();
        let node = PaneNode::leaf(a)
            .with_split(a, SplitDir::Horizontal, b, false)
            .with_split(b, SplitDir::Vertical, c, false);
        assert_eq!(node.leaves(), vec![a, b, c]);
    }

    #[test]
    fn split_on_missing_target_is_noop() {
        let a = tab();
        let node = PaneNode::leaf(a);
        let same = node.with_split(tab(), SplitDir::Horizontal, tab(), false);
        assert_eq!(same, node);
    }

    #[test]
    fn closing_a_leaf_promotes_its_sibling() {
        let a = tab();
        let b = tab();
        let node = PaneNode::leaf(a).with_split(a, SplitDir::Horizontal, b, false);
        let closed = node.with_closed_leaf(a).unwrap();
        assert_eq!(closed, PaneNode::leaf(b));
        assert!(closed.is_leaf());
    }

    #[test]
    fn closing_the_only_leaf_returns_none() {
        let a = tab();
        assert_eq!(PaneNode::leaf(a).with_closed_leaf(a), None);
    }

    #[test]
    fn closing_in_a_nested_tree_collapses_one_level() {
        let a = tab();
        let b = tab();
        let c = tab();
        // ((a | b) / c)
        let node = PaneNode::leaf(a)
            .with_split(a, SplitDir::Horizontal, b, false)
            .with_split(b, SplitDir::Vertical, c, false);
        let closed = node.with_closed_leaf(c).unwrap();
        assert_eq!(closed.leaves(), vec![a, b]);
        // closing b now leaves just a
        let closed2 = closed.with_closed_leaf(b).unwrap();
        assert_eq!(closed2, PaneNode::leaf(a));
    }

    #[test]
    fn replace_leaf_keeps_layout() {
        let a = tab();
        let b = tab();
        let c = tab();
        let node = PaneNode::leaf(a).with_split(a, SplitDir::Horizontal, b, false);
        let replaced = node.with_leaf_replaced(a, c);
        assert_eq!(replaced.leaves(), vec![c, b]);
    }

    #[test]
    fn ratio_is_set_and_clamped_at_path() {
        let a = tab();
        let b = tab();
        let node = PaneNode::leaf(a).with_split(a, SplitDir::Horizontal, b, false);
        let wide = node.with_ratio_at(&[], 900);
        match wide {
            PaneNode::Split { ratio, .. } => assert_eq!(ratio, 900),
            _ => panic!("expected split"),
        }
        // Clamp to the min/max guard.
        let clamped = node.with_ratio_at(&[], 0);
        match clamped {
            PaneNode::Split { ratio, .. } => assert_eq!(ratio, MIN_RATIO),
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn ratio_at_nested_path() {
        let a = tab();
        let b = tab();
        let c = tab();
        // (a | (b / c)): root split's second child is the b/c split.
        let node = PaneNode::leaf(a)
            .with_split(a, SplitDir::Horizontal, b, false)
            .with_split(b, SplitDir::Vertical, c, false);
        let updated = node.with_ratio_at(&[PaneBranch::Second], 700);
        match updated {
            PaneNode::Split { second, ratio, .. } => {
                assert_eq!(ratio, DEFAULT_RATIO, "root ratio unchanged");
                match *second {
                    PaneNode::Split { ratio, .. } => assert_eq!(ratio, 700),
                    _ => panic!("expected nested split"),
                }
            }
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn serde_round_trip() {
        let a = tab();
        let b = tab();
        let node = PaneNode::leaf(a).with_split(a, SplitDir::Vertical, b, false);
        let json = serde_json::to_string(&node).unwrap();
        let back: PaneNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node, back);
    }
}
