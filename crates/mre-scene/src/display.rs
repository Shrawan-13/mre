//! Immutable-ish display lists: the only thing the renderer consumes.
//!
//! Paint produces a flat list of commands. The backend sorts/groups it into
//! draw calls. Keeping this as a flat `Vec<Command>` rather than a tree of
//! visitors is deliberate: it is trivially reusable (cleared, not reallocated),
//! trivially diffable, and trivially GPU-batchable.

use mre_core::{Rect, Rgba8};

/// One drawing command.
///
/// Kept `Copy` and small so a display list is a flat array the CPU streams at
/// cache speed. Anything fat (glyph runs, image handles, paint servers) is
/// referenced by index into a side table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// A filled axis-aligned rectangle.
    Rect {
        /// Destination geometry in logical pixels.
        rect: Rect,
        /// Fill colour (premultiplied at upload time).
        color: Rgba8,
    },
    /// A shaped text run, referenced by id into the run cache.
    Text {
        /// Destination geometry in logical pixels.
        rect: Rect,
        /// Index into the shaped-run table.
        run: u32,
        /// Text colour.
        color: Rgba8,
    },
}

impl Command {
    /// The command's destination rect.
    #[inline]
    #[must_use]
    pub fn rect(&self) -> Rect {
        match *self {
            Self::Rect { rect, .. } | Self::Text { rect, .. } => rect,
        }
    }

    /// The command's colour.
    #[inline]
    #[must_use]
    pub fn color(&self) -> Rgba8 {
        match *self {
            Self::Rect { color, .. } | Self::Text { color, .. } => color,
        }
    }

    /// Batching key: commands sharing a key can share one draw call.
    ///
    /// Text and rects never batch together because they need different
    /// pipelines. Colour is the second axis today; once textures and clip
    /// stacks exist they join this key.
    #[inline]
    #[must_use]
    pub fn batch_key(&self) -> u64 {
        let kind = match self {
            Self::Rect { .. } => 0u64,
            Self::Text { .. } => 1u64,
        };
        (kind << 32) | u64::from(self.color().0)
    }
}

/// A reusable command buffer.
///
/// [`DisplayList::clear`] never frees, so a list reserved once during warm-up
/// is allocation-free forever. That is a hard requirement, not a nicety.
#[derive(Default)]
pub struct DisplayList {
    commands: Vec<Command>,
}

impl DisplayList {
    /// Pre-allocates room for `capacity` commands.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            commands: Vec::with_capacity(capacity),
        }
    }

    /// Empties the list, retaining capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    #[inline]
    pub fn push(&mut self, command: Command) {
        self.commands.push(command);
    }

    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.commands.capacity()
    }

    #[inline]
    #[must_use]
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Number of draw calls implied by this list, i.e. distinct batch keys.
    ///
    /// This is the lower bound a batcher can reach: one call per unique
    /// `(kind, colour)` pair. Allocation-free by using a fixed-size key table —
    /// if a scene ever produces more than 32 distinct keys in one frame, the
    /// returned value saturates and the batching gate will flag it.
    #[must_use]
    pub fn batch_count(&self) -> usize {
        const MAX_KEYS: usize = 32;
        let mut keys = [0u64; MAX_KEYS];
        let mut count = 0usize;
        for command in &self.commands {
            let key = command.batch_key();
            if !keys[..count].contains(&key) {
                if count == MAX_KEYS {
                    return count;
                }
                keys[count] = key;
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(color: Rgba8) -> Command {
        Command::Rect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            color,
        }
    }

    #[test]
    fn clear_retains_capacity() {
        let mut dl = DisplayList::with_capacity(128);
        let before = dl.capacity();
        for _ in 0..100 {
            dl.push(rect(Rgba8::WHITE));
        }
        dl.clear();
        assert!(dl.is_empty());
        assert_eq!(dl.capacity(), before, "clear must not release the buffer");
    }

    #[test]
    fn batching_counts_distinct_keys() {
        let mut dl = DisplayList::with_capacity(16);
        dl.push(rect(Rgba8::WHITE));
        dl.push(rect(Rgba8::WHITE));
        dl.push(rect(Rgba8::BLACK));
        dl.push(Command::Text {
            rect: Rect::EMPTY,
            run: 0,
            color: Rgba8::WHITE,
        });
        assert_eq!(dl.batch_count(), 3);
    }

    #[test]
    fn command_stays_small() {
        // A display list is streamed linearly; if commands get fat, painting
        // gets memory-bandwidth bound. 32 bytes keeps two per cache line.
        assert!(core::mem::size_of::<Command>() <= 32);
    }
}
