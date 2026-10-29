//! Scene nodes.

use crate::dirty::Dirty;
use mre_core::{Id, Rect};

/// What a node is for. Drives which layout and paint rules apply.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum NodeKind {
    /// The scene root. Exactly one exists.
    Root,
    /// A sticky table header.
    Header,
    /// The scrolling body region.
    Body,
    /// One virtualised row.
    Row,
    /// A cell inside a row. Reserved for when layout gets real.
    Cell,
}

/// One node in the retained tree.
///
/// # Layout
///
/// ```text
/// kind       1 byte   \
/// dirty      1 byte    |  4
/// (padding)  2 bytes  /
/// parent        8  \
/// first_child   8   |
/// last_child    8   |  40  (five links, all Option<Id> = 8 bytes thanks to
/// next_sibling  8   |       the NonZeroU32 generation niche)
/// prev_sibling  8  /
/// bounds       16  (Rect = four f32)
/// payload       4  (row index / text run id)
/// -----------------
///               64 bytes == exactly one cache line
/// ```
///
/// This is not accidental: it means walking the tree costs one cache line per
/// node, and it is enforced by a test. Links are `Option<Id>` rather than
/// `Id` plus a sentinel so an invalid link cannot be constructed by accident.
#[derive(Clone, Debug)]
pub struct Node {
    pub kind: NodeKind,
    pub dirty: Dirty,
    /// Parent, or `None` for the root.
    pub parent: Option<Id>,
    /// First child, or `None` when leaf.
    pub first_child: Option<Id>,
    /// Last child. Gives O(1) append to a child list.
    pub last_child: Option<Id>,
    /// Next sibling, or `None` at the end of a child list.
    pub next_sibling: Option<Id>,
    /// Previous sibling. Gives O(1) unlink from a child list.
    pub prev_sibling: Option<Id>,
    /// Laid-out bounds in logical pixels, relative to the window.
    pub bounds: Rect,
    /// Kind-specific index: row number for [`NodeKind::Row`], text run id otherwise.
    pub payload: u32,
}

impl Node {
    /// A brand-new node is fully dirty: nothing is known about it yet.
    pub const fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            dirty: Dirty::ALL,
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            prev_sibling: None,
            bounds: Rect::EMPTY,
            payload: 0,
        }
    }

    /// True when the node has no children.
    #[inline]
    #[must_use]
    pub const fn is_leaf(&self) -> bool {
        self.first_child.is_none()
    }

    /// Clears every dirty flag. Called once the node's work is done.
    #[inline]
    pub fn clean(&mut self) {
        self.dirty = Dirty::NONE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn node_is_exactly_one_cache_line() {
        // If this fails, the tree has stopped being cache-linear and every
        // traversal in the engine just got measurably slower.
        assert_eq!(size_of::<Node>(), mre_core::CACHE_LINE);
    }

    #[test]
    fn new_node_is_fully_dirty() {
        let n = Node::new(NodeKind::Row);
        assert_eq!(n.dirty, Dirty::ALL);
        assert!(n.is_leaf());
    }
}
