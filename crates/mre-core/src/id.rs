//! Generational ids.

use core::fmt;
use core::num::NonZeroU32;

/// A generational handle into an [`Arena`](crate::Arena).
///
/// ```text
/// layout: [ index: u32 ][ generation: NonZeroU32 ]   = 8 bytes
/// ```
///
/// The generation is [`NonZeroU32`], which is what makes `Option<Id>` also 8
/// bytes wide (niche optimisation). That matters a lot: the scene node stores
/// five such links, and without the niche those five links would cost 60 bytes
/// instead of 40.
///
/// An `Id` is only meaningful for the [`Arena`](crate::Arena) that produced it.
/// Feeding it to another arena is a logic error that resolves to `None` on
/// account of the generation check.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id {
    pub(crate) index: u32,
    pub(crate) generation: NonZeroU32,
}

impl Id {
    /// Builds an id. Crate-internal: ids must only originate from an arena.
    #[inline]
    pub(crate) const fn new(index: u32, generation: NonZeroU32) -> Self {
        Self { index, generation }
    }

    /// Slot index in the arena's backing storage.
    #[inline]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot at the time this id was issued.
    ///
    /// A slot whose generation differs from this id's generation has been
    /// removed and possibly reused, so this id is dead.
    #[inline]
    pub const fn generation(self) -> u32 {
        self.generation.get()
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({}v{})", self.index, self.generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn id_is_eight_bytes() {
        assert_eq!(size_of::<Id>(), 8);
    }

    #[test]
    fn option_id_is_eight_bytes() {
        // This is a load-bearing property, not a curiosity: node links rely on it.
        assert_eq!(size_of::<Option<Id>>(), 8);
    }
}
