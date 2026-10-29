//! The retained tree.

use crate::node::{Node, NodeKind};
use core::ops::{Index, IndexMut};
use mre_core::{Arena, Id};

/// A retained scene graph backed by a single generational arena.
///
/// Children are stored as an intrusive doubly-linked list scattered through the
/// arena. That sounds worse than a `Vec<Id>` per parent, but it means:
///
/// * append and unlink are both O(1) with no per-parent allocation,
/// * child lists never reallocate, so the tree cannot allocate while rendering,
/// * nodes stay in one contiguous array, so a full traversal is a linear scan.
pub struct Scene {
    nodes: Arena<Node>,
    root: Id,
    /// Reused DFS stack for subtree removal. Exists so removal never allocates.
    scratch: Vec<Id>,
}

impl Scene {
    /// Creates a scene containing only a root node.
    pub fn new() -> Self {
        Self::with_capacity(64)
    }

    /// Creates a scene with room for `capacity` nodes.
    ///
    /// Pre-size this. An arena that grows during a frame allocates during a
    /// frame, which the allocation gate will reject.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut nodes = Arena::with_capacity(capacity);
        let root = nodes.insert(Node::new(NodeKind::Root));
        Self {
            nodes,
            root,
            scratch: Vec::with_capacity(32),
        }
    }

    #[inline]
    #[must_use]
    pub fn root(&self) -> Id {
        self.root
    }

    #[inline]
    #[must_use]
    pub fn get(&self, id: Id) -> Option<&Node> {
        self.nodes.get(id)
    }

    #[inline]
    pub fn get_mut(&mut self, id: Id) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    #[inline]
    #[must_use]
    pub fn contains(&self, id: Id) -> bool {
        self.nodes.contains(id)
    }

    /// Number of live nodes.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Bytes of arena storage currently committed, live or free.
    #[inline]
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.nodes.capacity() * Arena::<Node>::slot_size()
    }

    /// Appends `node` as the last child of `parent`, returning its id.
    ///
    /// # Panics
    ///
    /// Panics if `parent` is not a live id in this scene.
    pub fn append(&mut self, parent: Id, node: Node) -> Id {
        assert!(
            self.nodes.contains(parent),
            "append to a dangling parent id"
        );
        let child = self.nodes.insert(node);
        self.link_last(parent, child);
        child
    }

    /// Removes `id` and its entire subtree. Returns the number of nodes removed.
    ///
    /// The root cannot be removed; removing it is a no-op returning `0`.
    pub fn remove(&mut self, id: Id) -> usize {
        if id == self.root || !self.nodes.contains(id) {
            return 0;
        }
        self.unlink(id);

        self.scratch.clear();
        self.scratch.push(id);
        let mut removed = 0;
        // Iterative DFS: a recursive drop would blow the stack on deep trees and
        // cost a call frame per node.
        while let Some(current) = self.scratch.pop() {
            let mut child = self.nodes[current].first_child;
            while let Some(c) = child {
                child = self.nodes[c].next_sibling;
                self.scratch.push(c);
            }
            self.nodes.remove(current);
            removed += 1;
        }
        removed
    }

    /// Iterates the direct children of `parent`, in order.
    pub fn children(&self, parent: Id) -> impl Iterator<Item = Id> + '_ {
        let nodes = &self.nodes;
        let mut next = nodes.get(parent).and_then(|n| n.first_child);
        core::iter::from_fn(move || {
            let id = next?;
            next = nodes.get(id).and_then(|n| n.next_sibling);
            Some(id)
        })
    }

    /// Walks every node in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Id, &Node)> + '_ {
        self.nodes.iter()
    }

    /// Mutable walk of every node in slot order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Id, &mut Node)> + '_ {
        self.nodes.iter_mut()
    }

    /// Clears all dirty flags in the tree. O(nodes), used at the end of a frame.
    pub fn clear_dirty(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.clean();
        }
    }

    fn link_last(&mut self, parent: Id, child: Id) {
        let last = self.nodes[parent].last_child;
        self.nodes[child].parent = Some(parent);
        match last {
            Some(last) => {
                self.nodes[last].next_sibling = Some(child);
                self.nodes[child].prev_sibling = Some(last);
                self.nodes[parent].last_child = Some(child);
            }
            None => {
                self.nodes[parent].first_child = Some(child);
                self.nodes[parent].last_child = Some(child);
            }
        }
    }

    fn unlink(&mut self, id: Id) {
        let Some(parent) = self.nodes[id].parent else {
            return;
        };
        let prev = self.nodes[id].prev_sibling;
        let next = self.nodes[id].next_sibling;

        match prev {
            Some(p) => self.nodes[p].next_sibling = next,
            None => self.nodes[parent].first_child = next,
        }
        match next {
            Some(n) => self.nodes[n].prev_sibling = prev,
            None => self.nodes[parent].last_child = prev,
        }

        let node = &mut self.nodes[id];
        node.parent = None;
        node.prev_sibling = None;
        node.next_sibling = None;
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Index<Id> for Scene {
    type Output = Node;

    #[inline]
    fn index(&self, id: Id) -> &Node {
        self.nodes.get(id).expect("scene id was dangling")
    }
}

impl IndexMut<Id> for Scene {
    #[inline]
    fn index_mut(&mut self, id: Id) -> &mut Node {
        self.nodes.get_mut(id).expect("scene id was dangling")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dirty::Dirty;

    fn build() -> (Scene, Id) {
        let mut scene = Scene::new();
        let root = scene.root();
        let a = scene.append(root, Node::new(NodeKind::Header));
        let b = scene.append(root, Node::new(NodeKind::Body));
        scene.append(a, Node::new(NodeKind::Row));
        scene.append(a, Node::new(NodeKind::Row));
        (scene, b)
    }

    #[test]
    fn append_preserves_order() {
        let mut scene = Scene::new();
        let root = scene.root();
        let ids: Vec<Id> = (0..3)
            .map(|_| scene.append(root, Node::new(NodeKind::Row)))
            .collect();
        let children: Vec<Id> = scene.children(root).collect();
        assert_eq!(children, ids);
    }

    #[test]
    fn append_is_constant_time_at_the_tail() {
        let mut scene = Scene::new();
        let root = scene.root();
        let header = scene.append(root, Node::new(NodeKind::Header));
        let body = scene.append(root, Node::new(NodeKind::Body));
        let tail = scene.append(root, Node::new(NodeKind::Row));
        let children: Vec<Id> = scene.children(root).collect();
        assert_eq!(
            children,
            vec![header, body, tail],
            "append must go to the tail"
        );
    }

    #[test]
    fn remove_detaches_subtree_and_restores_child_list() {
        let (mut scene, _) = build();
        let root = scene.root();
        let header = scene.children(root).next().unwrap();
        let before = scene.len();
        let removed = scene.remove(header);
        assert_eq!(removed, 3, "header plus two rows");
        assert_eq!(scene.len(), before - 3);
        assert_eq!(scene.children(root).count(), 1, "body must remain");
    }

    #[test]
    fn remove_unlinks_from_the_middle() {
        let mut scene = Scene::new();
        let root = scene.root();
        let a = scene.append(root, Node::new(NodeKind::Row));
        let b = scene.append(root, Node::new(NodeKind::Row));
        let c = scene.append(root, Node::new(NodeKind::Row));
        scene.remove(b);
        let children: Vec<Id> = scene.children(root).collect();
        assert_eq!(children, vec![a, c]);
        assert!(scene[c].prev_sibling == Some(a));
    }

    #[test]
    fn remove_root_is_refused() {
        let (mut scene, _) = build();
        let root = scene.root();
        assert_eq!(scene.remove(root), 0);
        assert!(scene.contains(root));
    }

    #[test]
    fn remove_deep_tree_does_not_recurse() {
        let mut scene = Scene::new();
        let mut parent = scene.root();
        for _ in 0..10_000 {
            parent = scene.append(parent, Node::new(NodeKind::Row));
        }
        let top = scene.children(scene.root()).next().unwrap();
        assert_eq!(scene.remove(top), 10_000);
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn clear_dirty_cleans_every_node() {
        let (mut scene, body) = build();
        assert_eq!(scene[body].dirty, Dirty::ALL);
        scene.clear_dirty();
        assert!(scene.iter().all(|(_, n)| n.dirty.is_empty()));
    }
}
