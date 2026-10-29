//! The layout tree.
//!
//! A flat arena of nodes with child index lists. It is separate from the scene
//! tree because layout runs on a different clock: the scene keeps paint state,
//! layout keeps geometry, and neither has to know the other's invariants.
//!
//! Nodes are stable handles. Removing a subtree detaches it but does not reuse
//! its id, so a stale handle can never silently address a different node.

use crate::style::Style;

/// A handle to a layout node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// The node's index in the arena.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The raw index.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A handle to a text run measured by a [`crate::TextMetrics`] implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextId(pub u32);

/// What a node contains, beyond its children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Content {
    /// Nothing; the node is a pure container.
    #[default]
    None,
    /// A text run, measured externally.
    Text(TextId),
}

/// One node in the layout tree.
#[derive(Clone, Debug)]
pub struct LayoutNode {
    /// Layout properties.
    pub style: Style,
    /// Children, in paint order.
    pub children: Vec<NodeId>,
    /// The node's own content.
    pub content: Content,
    /// Parent, or `None` for the root or a detached node.
    pub parent: Option<NodeId>,
    /// Cached geometry from the last layout, for hit testing and paint.
    pub bounds: mre_core::Rect,
}

/// A tree of layout nodes.
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<LayoutNode>,
    root: NodeId,
}

impl Tree {
    /// Creates a tree with a single root node.
    #[must_use]
    pub fn new(root_style: Style) -> Self {
        Self {
            nodes: vec![LayoutNode {
                style: root_style,
                children: Vec::new(),
                content: Content::None,
                parent: None,
                bounds: mre_core::Rect::EMPTY,
            }],
            root: NodeId(0),
        }
    }

    /// The root node.
    #[must_use]
    pub const fn root(&self) -> NodeId {
        self.root
    }

    /// Number of nodes, including detached ones.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True when the tree has no nodes. Never true in practice: the root always
    /// exists, so this is here to satisfy the lint rather than to be used.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Appends a child with no content.
    pub fn add_child(&mut self, parent: NodeId, style: Style) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).expect("layout tree exceeds 4 GiB nodes"));
        self.nodes.push(LayoutNode {
            style,
            children: Vec::new(),
            content: Content::None,
            parent: Some(parent),
            bounds: mre_core::Rect::EMPTY,
        });
        self.nodes[parent.index()].children.push(id);
        id
    }

    /// Appends a text child.
    pub fn add_text(&mut self, parent: NodeId, style: Style, text: TextId) -> NodeId {
        let id = self.add_child(parent, style);
        self.nodes[id.index()].content = Content::Text(text);
        id
    }

    /// Borrows a node.
    ///
    /// # Panics
    ///
    /// Panics if the handle does not refer to a node in this tree.
    #[must_use]
    pub fn node(&self, id: NodeId) -> &LayoutNode {
        &self.nodes[id.index()]
    }

    /// Mutably borrows a node.
    ///
    /// # Panics
    ///
    /// Panics if the handle does not refer to a node in this tree.
    pub fn node_mut(&mut self, id: NodeId) -> &mut LayoutNode {
        &mut self.nodes[id.index()]
    }

    /// Replaces a node's style.
    pub fn set_style(&mut self, id: NodeId, style: Style) {
        self.nodes[id.index()].style = style;
    }

    /// A node's children.
    ///
    /// # Panics
    ///
    /// Panics if the handle does not refer to a node in this tree.
    #[must_use]
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.nodes[id.index()].children
    }

    /// A node's parent.
    #[must_use]
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.index()).and_then(|node| node.parent)
    }

    /// Detaches a subtree. The nodes remain allocated, so existing handles stay
    /// valid and resolve to a detached subtree rather than aliasing a new node.
    pub fn remove(&mut self, id: NodeId) {
        if id == self.root {
            return;
        }
        if let Some(parent) = self.nodes[id.index()].parent {
            self.nodes[parent.index()]
                .children
                .retain(|child| *child != id);
        }
        self.nodes[id.index()].parent = None;
    }

    /// Iterates over a subtree, parents before children.
    pub fn preorder(&self, id: NodeId) -> Preorder<'_> {
        Preorder {
            tree: self,
            stack: vec![id],
        }
    }
}

/// Depth-first, parents-first iterator.
pub struct Preorder<'a> {
    tree: &'a Tree,
    stack: Vec<NodeId>,
}

impl Iterator for Preorder<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        for child in self.tree.children(id).iter().rev() {
            self.stack.push(*child);
        }
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_are_appended_in_order() {
        let mut tree = Tree::new(Style::default());
        let root = tree.root();
        let a = tree.add_child(root, Style::default());
        let b = tree.add_child(root, Style::default());
        assert_eq!(tree.children(root), &[a, b]);
        assert_eq!(tree.parent(a), Some(root));
        assert_eq!(tree.parent(root), None);
    }

    #[test]
    fn preorder_is_parents_first() {
        let mut tree = Tree::new(Style::default());
        let root = tree.root();
        let a = tree.add_child(root, Style::default());
        let b = tree.add_child(root, Style::default());
        let a1 = tree.add_child(a, Style::default());
        let order: Vec<NodeId> = tree.preorder(root).collect();
        assert_eq!(order, vec![root, a, a1, b]);
    }

    #[test]
    fn removing_detaches_without_invalidating_siblings() {
        let mut tree = Tree::new(Style::default());
        let root = tree.root();
        let a = tree.add_child(root, Style::default());
        let b = tree.add_child(root, Style::default());
        tree.remove(a);
        assert_eq!(tree.children(root), &[b]);
        assert_eq!(tree.parent(a), None);
        // The detached node still exists and keeps its own identity.
        assert_eq!(tree.node(a).children.len(), 0);
    }

    #[test]
    fn removing_the_root_is_a_no_op() {
        let mut tree = Tree::new(Style::default());
        let root = tree.root();
        tree.remove(root);
        assert_eq!(tree.root(), root);
        assert_eq!(tree.parent(root), None);
    }
}
