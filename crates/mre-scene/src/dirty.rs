//! Dirty flags: the mechanism that makes "do nothing" free.

use core::ops::{BitOr, BitOrAssign};

/// A set of pending-work flags for one node.
///
/// The rule that keeps the engine fast is simple and absolute: **work is only
/// performed for nodes whose relevant flag is set.** A node with
/// `dirty == NONE` is skipped by every stage. That is what makes an unchanged
/// subtree cost zero, and it is why retained mode can beat immediate mode by
/// an order of magnitude on incremental frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Dirty(u8);

impl Dirty {
    /// Nothing to do. The common case in a steady-state app.
    pub const NONE: Self = Self(0);
    /// Bounds or children changed; the subtree must be laid out again.
    pub const LAYOUT: Self = Self(1 << 0);
    /// Visual output changed; the display list must be rebuilt for this node.
    pub const PAINT: Self = Self(1 << 1);
    /// Text content changed; shaped-run caches must be invalidated.
    pub const TEXT: Self = Self(1 << 2);
    /// Every flag.
    pub const ALL: Self = Self(0b111);

    #[inline]
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// True when *all* of `other`'s flags are present.
    #[inline]
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when *any* of `other`'s flags are present.
    #[inline]
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    #[inline]
    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    #[inline]
    pub fn remove(&mut self, other: Self) {
        self.0 &= !other.0;
    }

    /// Raw bits, for serialisation and debugging.
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Reconstructs a set from raw bits.
    #[inline]
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }
}

impl BitOr for Dirty {
    type Output = Self;

    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Dirty {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_compose() {
        let mut d = Dirty::NONE;
        assert!(d.is_empty());
        d |= Dirty::LAYOUT | Dirty::PAINT;
        assert!(d.contains(Dirty::LAYOUT));
        assert!(d.contains(Dirty::PAINT));
        assert!(!d.contains(Dirty::TEXT));
        assert!(d.intersects(Dirty::PAINT));
    }

    #[test]
    fn removal_is_precise() {
        let mut d = Dirty::ALL;
        d.remove(Dirty::LAYOUT);
        assert!(!d.contains(Dirty::LAYOUT));
        assert!(d.contains(Dirty::PAINT));
        assert!(d.contains(Dirty::TEXT));
    }

    #[test]
    fn fits_in_a_byte_so_nodes_stay_one_cache_line() {
        assert_eq!(core::mem::size_of::<Dirty>(), 1);
    }
}
