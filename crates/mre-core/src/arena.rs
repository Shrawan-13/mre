//! A generational arena: the only container the scene tree uses.

use crate::id::Id;
use core::num::NonZeroU32;
use core::ops::{Index, IndexMut};

struct Entry<T> {
    /// A removed slot keeps its (incremented) generation so stale ids can never
    /// alias the slot's next occupant.
    generation: NonZeroU32,
    value: Option<T>,
}

/// Dense, generational storage with O(1) insert, remove and lookup.
///
/// Why not `Rc<RefCell<Node>>` like every tutorial does?
///
/// * `Rc` boxes every node separately: one pointer chase and one cache miss per
///   traversal, and 16 bytes of refcount overhead per node.
/// * `RefCell` borrow checks sit on the hot path.
/// * A tree of `Rc` cannot be iterated in address order, so it defeats every
///   prefetching strategy.
///
/// The arena keeps nodes contiguous (iteration is a linear scan) while still
/// allowing interior mutation of any node at any time.
///
/// # Complexity
///
/// | op | cost |
/// |----|------|
/// | `insert` | O(1) amortised |
/// | `remove` | O(1) |
/// | `get` | O(1) |
/// | `iter` | O(capacity) |
pub struct Arena<T> {
    entries: Vec<Entry<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Arena<T> {
    /// An empty arena. Does not allocate until the first insert.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    /// Pre-sizes both the slot array and the free list.
    ///
    /// Pre-sizing is a performance contract, not a nicety: an arena that grows
    /// while rendering allocates while rendering, which is exactly what the
    /// zero-allocation gate forbids.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            // A full-capacity `free` list can only happen after removing
            // everything, so sizing it to `capacity` is the worst case.
            free: Vec::with_capacity(capacity),
            len: 0,
        }
    }

    #[inline]
    /// Inserts a value, reusing a free slot when one is available.
    ///
    /// # Panics
    ///
    /// Panics if the arena would exceed `u32::MAX` slots.
    pub fn insert(&mut self, value: T) -> Id {
        if let Some(index) = self.free.pop() {
            let entry = &mut self.entries[index as usize];
            debug_assert!(
                entry.value.is_none(),
                "free list pointed at an occupied slot"
            );
            entry.value = Some(value);
            self.len += 1;
            Id::new(index, entry.generation)
        } else {
            let index = u32::try_from(self.entries.len()).expect("arena exceeded u32::MAX slots");
            let generation = NonZeroU32::MIN;
            self.entries.push(Entry {
                generation,
                value: Some(value),
            });
            self.len += 1;
            Id::new(index, generation)
        }
    }

    #[inline]
    /// Removes and returns the value, invalidating `id`.
    ///
    /// Returns `None` when `id` is already dead (stale generation).
    pub fn remove(&mut self, id: Id) -> Option<T> {
        let entry = self.entries.get_mut(id.index as usize)?;
        if entry.generation != id.generation {
            return None;
        }
        let value = entry.value.take()?;
        entry.generation = Self::next_generation(entry.generation);
        self.free.push(id.index);
        self.len -= 1;
        Some(value)
    }

    #[inline]
    /// Borrows the value behind `id`, or `None` if `id` is stale.
    pub fn get(&self, id: Id) -> Option<&T> {
        self.entries
            .get(id.index as usize)
            .filter(|e| e.generation == id.generation)
            .and_then(|e| e.value.as_ref())
    }

    #[inline]
    /// Mutably borrows the value behind `id`, or `None` if `id` is stale.
    pub fn get_mut(&mut self, id: Id) -> Option<&mut T> {
        self.entries
            .get_mut(id.index as usize)
            .filter(|e| e.generation == id.generation)
            .and_then(|e| e.value.as_mut())
    }

    /// True when `id` refers to a live slot.
    #[inline]
    /// True when `id` refers to a live slot.
    pub fn contains(&self, id: Id) -> bool {
        self.get(id).is_some()
    }

    /// Drops every value and invalidates every outstanding id.
    ///
    /// O(capacity), not O(len), because the generations must be bumped.
    pub fn clear(&mut self) {
        for (index, entry) in self.entries.iter_mut().enumerate() {
            if entry.value.take().is_some() {
                entry.generation = Self::next_generation(entry.generation);
                self.free.push(index as u32);
            }
        }
        self.len = 0;
    }

    #[inline]
    /// Number of live values.
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    /// True when no values are live.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Number of allocated slots, live or free. This is what costs memory.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// Bytes occupied by one slot, including arena bookkeeping.
    ///
    /// Used by memory gates: `capacity() * slot_size()` is the arena's real
    /// footprint, not the sum of the values inside it.
    #[inline]
    #[must_use]
    pub const fn slot_size() -> usize {
        core::mem::size_of::<Entry<T>>()
    }

    /// Live entries with their ids, in slot order (≈ address order).
    pub fn iter(&self) -> impl Iterator<Item = (Id, &T)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                entry
                    .value
                    .as_ref()
                    .map(|value| (Id::new(index as u32, entry.generation), value))
            })
    }

    /// Mutable variant of [`Arena::iter`].
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Id, &mut T)> + '_ {
        self.entries
            .iter_mut()
            .enumerate()
            .filter_map(|(index, entry)| {
                let generation = entry.generation;
                entry
                    .value
                    .as_mut()
                    .map(|value| (Id::new(index as u32, generation), value))
            })
    }

    /// Wraps a generation, skipping zero (which is reserved for "no id").
    #[inline]
    fn next_generation(generation: NonZeroU32) -> NonZeroU32 {
        NonZeroU32::new(generation.get().wrapping_add(1)).unwrap_or(NonZeroU32::MIN)
    }
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Index<Id> for Arena<T> {
    type Output = T;

    #[inline]
    fn index(&self, id: Id) -> &T {
        self.get(id).expect("arena id was dangling")
    }
}

impl<T> IndexMut<Id> for Arena<T> {
    #[inline]
    fn index_mut(&mut self, id: Id) -> &mut T {
        self.get_mut(id).expect("arena id was dangling")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_remove() {
        let mut a = Arena::new();
        let x = a.insert(10u32);
        let y = a.insert(20u32);
        assert_eq!(a[x], 10);
        assert_eq!(a[y], 20);
        assert_eq!(a.len(), 2);

        assert_eq!(a.remove(x), Some(10));
        assert_eq!(a.get(x), None, "removed id must be dead");
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn stale_id_cannot_alias_recycled_slot() {
        let mut a = Arena::new();
        let first = a.insert(1u8);
        a.remove(first);
        let second = a.insert(2u8);
        assert_eq!(first.index(), second.index(), "slot should be recycled");
        assert_ne!(first.generation(), second.generation());
        assert_eq!(a.get(first), None);
        assert_eq!(a.get(second), Some(&2));
    }

    #[test]
    fn removal_recycles_slot_and_keeps_density() {
        let mut a = Arena::with_capacity(8);
        let ids: Vec<Id> = (0..8).map(|i| a.insert(i as u32)).collect();
        for id in &ids[..4] {
            a.remove(*id);
        }
        for i in 0..4 {
            a.insert(100 + i);
        }
        // No growth: the free list served every insert.
        assert_eq!(a.capacity(), 8);
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn clear_invalidates_all_ids() {
        let mut a = Arena::new();
        let ids: Vec<Id> = (0..4).map(|i| a.insert(i)).collect();
        a.clear();
        assert!(a.is_empty());
        for id in ids {
            assert!(!a.contains(id));
        }
        // Reinserting must not resurrect old ids.
        let fresh = a.insert(99);
        assert_eq!(a[fresh], 99);
    }

    #[test]
    fn iter_visits_only_live_slots() {
        let mut a = Arena::new();
        let ids: Vec<Id> = (0..5).map(|i| a.insert(i)).collect();
        a.remove(ids[1]);
        a.remove(ids[3]);
        let mut seen: Vec<u32> = a.iter().map(|(_, v)| *v).collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 2, 4]);
    }

    #[test]
    fn cross_arena_id_is_rejected() {
        let mut a: Arena<u8> = Arena::new();
        let b: Arena<u8> = Arena::new();
        let id = a.insert(1);
        // Same index and generation, different arena: must not resolve.
        assert_eq!(b.get(id), None);
    }
}
