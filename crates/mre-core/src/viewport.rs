//! Viewports, density and responsive breakpoints.
//!
//! Rotation, window resize and multi-window are the same event to the engine:
//! the logical viewport changed. Everything above this module lays out from
//! `Viewport`, so there is exactly one place where physical pixels become
//! logical ones.

use crate::geom::{Rect, Size};

/// Distance from each edge of the window, in logical pixels.
///
/// Notches, rounded corners, status bars, navigation bars and on-screen
/// keyboards all arrive here. Content that must stay visible should be laid out
/// inside [`Viewport::content`] rather than the full window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    /// Distance from the top edge.
    pub top: f32,
    /// Distance from the right edge.
    pub right: f32,
    /// Distance from the bottom edge.
    pub bottom: f32,
    /// Distance from the left edge.
    pub left: f32,
}

impl Insets {
    /// No insets.
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    /// Uniform insets on all edges.
    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Insets for a device with a notch or a status bar.
    #[must_use]
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Total horizontal inset.
    #[must_use]
    pub const fn horizontal(self) -> f32 {
        self.left + self.right
    }

    /// Total vertical inset.
    #[must_use]
    pub const fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

/// Whether the viewport is wider or taller than it is high or wide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Orientation {
    /// Height greater than or equal to width.
    Portrait,
    /// Width greater than height.
    Landscape,
}

impl Orientation {
    /// Classifies a logical size.
    #[must_use]
    pub fn of(size: Size) -> Self {
        if size.w > size.h {
            Self::Landscape
        } else {
            Self::Portrait
        }
    }

    /// The orientation after a rotation.
    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::Portrait => Self::Landscape,
            Self::Landscape => Self::Portrait,
        }
    }
}

/// A window or surface, in logical pixels, with its density and insets.
///
/// `scale` converts logical to physical: `physical = logical * scale`. Layout
/// never sees physical pixels, so a 2x display and a 1x display produce the same
/// geometry and only the rasteriser differs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// Logical size of the surface.
    pub size: Size,
    /// Physical pixels per logical pixel. `1.0`, `1.5`, `2.0`, `2.75`, ...
    pub scale: f32,
    /// Safe-area insets, logical pixels.
    pub insets: Insets,
}

impl Viewport {
    /// A viewport with no insets and unit scale.
    #[must_use]
    pub const fn new(size: Size) -> Self {
        Self {
            size,
            scale: 1.0,
            insets: Insets::ZERO,
        }
    }

    /// Creates a viewport from a physical size and a scale factor.
    ///
    /// This is the constructor platform backends should use: window managers
    /// report physical pixels, the engine thinks in logical ones.
    #[must_use]
    pub fn from_physical(physical: Size, scale: f32) -> Self {
        let scale = if scale > 0.0 { scale } else { 1.0 };
        Self {
            size: Size::new(physical.w / scale, physical.h / scale),
            scale,
            insets: Insets::ZERO,
        }
    }

    /// Sets the safe-area insets.
    #[must_use]
    pub const fn with_insets(mut self, insets: Insets) -> Self {
        self.insets = insets;
        self
    }

    /// Sets the density scale.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = if scale > 0.0 { scale } else { 1.0 };
        self
    }

    /// Current orientation.
    #[must_use]
    pub fn orientation(&self) -> Orientation {
        Orientation::of(self.size)
    }

    /// The physical size of the surface, rounded to whole pixels.
    #[must_use]
    pub fn physical(&self) -> Size {
        Size::new(
            (self.size.w * self.scale).round(),
            (self.size.h * self.scale).round(),
        )
    }

    /// The rectangle available to content after safe-area insets.
    ///
    /// Clamped to a non-negative size, because a keyboard inset larger than the
    /// window must not produce a negative layout.
    #[must_use]
    pub fn content(&self) -> Rect {
        let w = (self.size.w - self.insets.horizontal()).max(0.0);
        let h = (self.size.h - self.insets.vertical()).max(0.0);
        Rect::new(self.insets.left, self.insets.top, w, h)
    }

    /// Resizes in logical units, as on a window resize.
    pub fn resize(&mut self, size: Size) {
        self.size = size;
    }

    /// Applies a rotation: swaps width and height and swaps the horizontal and
    /// vertical safe-area insets.
    ///
    /// This is the only correct way to handle rotation. A layout that simply
    /// receives a swapped size will otherwise keep a top inset where a left
    /// inset now belongs.
    pub fn rotate(&mut self) {
        self.size = Size::new(self.size.h, self.size.w);
        let insets = self.insets;
        self.insets = Insets::new(insets.left, insets.top, insets.right, insets.bottom);
    }

    /// Classifies the width into a size class.
    #[must_use]
    pub fn width_class(&self, breakpoints: &Breakpoints) -> SizeClass {
        breakpoints.classify(self.size.w)
    }
}

/// A coarse width class, for picking a layout rather than a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SizeClass {
    /// Phones in portrait, narrow side panels.
    Compact,
    /// Tablets, small windows, split-screen.
    Medium,
    /// Desktop windows, tablets in landscape.
    Expanded,
}

impl SizeClass {
    /// A stable index, for indexing tables of layout values.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Compact => 0,
            Self::Medium => 1,
            Self::Expanded => 2,
        }
    }

    /// Picks a value for the current class, falling back to the nearest defined
    /// class so callers do not have to specify all three.
    #[must_use]
    pub fn pick<T: Copy>(self, compact: T, medium: Option<T>, expanded: Option<T>) -> T {
        match self {
            Self::Compact => compact,
            Self::Medium => medium.or(expanded).unwrap_or(compact),
            Self::Expanded => expanded.or(medium).unwrap_or(compact),
        }
    }
}

/// Width thresholds between [`SizeClass`]es, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Breakpoints {
    /// At or above this width the class is [`SizeClass::Medium`].
    pub medium: f32,
    /// At or above this width the class is [`SizeClass::Expanded`].
    pub expanded: f32,
}

impl Breakpoints {
    /// Material-style defaults: 600 / 840 logical pixels.
    pub const DEFAULT: Self = Self {
        medium: 600.0,
        expanded: 840.0,
    };

    /// Custom thresholds.
    #[must_use]
    pub const fn new(medium: f32, expanded: f32) -> Self {
        Self { medium, expanded }
    }

    /// Classifies a width.
    #[must_use]
    pub fn classify(&self, width: f32) -> SizeClass {
        if width >= self.expanded {
            SizeClass::Expanded
        } else if width >= self.medium {
            SizeClass::Medium
        } else {
            SizeClass::Compact
        }
    }
}

impl Default for Breakpoints {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_and_logical_are_inverses() {
        let vp = Viewport::from_physical(Size::new(2560.0, 1440.0), 2.0);
        assert_eq!(vp.size, Size::new(1280.0, 720.0));
        assert_eq!(vp.physical(), Size::new(2560.0, 1440.0));
        assert_eq!(vp.orientation(), Orientation::Landscape);
    }

    #[test]
    fn rotation_swaps_size_and_insets() {
        let mut vp =
            Viewport::new(Size::new(400.0, 800.0)).with_insets(Insets::new(44.0, 0.0, 34.0, 0.0));
        assert_eq!(vp.orientation(), Orientation::Portrait);

        vp.rotate();

        assert_eq!(vp.size, Size::new(800.0, 400.0));
        assert_eq!(vp.orientation(), Orientation::Landscape);
        // The top inset moves to the left, the bottom inset to the right.
        assert_eq!(vp.insets, Insets::new(0.0, 44.0, 0.0, 34.0));
    }

    #[test]
    fn content_rect_is_clamped_when_insets_exceed_the_window() {
        let vp =
            Viewport::new(Size::new(320.0, 200.0)).with_insets(Insets::new(0.0, 0.0, 500.0, 0.0));
        assert_eq!(vp.content(), Rect::new(0.0, 0.0, 320.0, 0.0));
    }

    #[test]
    fn breakpoints_classify_edges() {
        let b = Breakpoints::DEFAULT;
        assert_eq!(b.classify(599.0), SizeClass::Compact);
        assert_eq!(b.classify(600.0), SizeClass::Medium);
        assert_eq!(b.classify(840.0), SizeClass::Expanded);
        assert_eq!(b.classify(4000.0), SizeClass::Expanded);
    }

    #[test]
    fn size_class_falls_back_to_the_nearest_defined_value() {
        // A table that defines compact + expanded only.
        let value = |c: SizeClass| c.pick(1, None, Some(3));
        assert_eq!(value(SizeClass::Compact), 1);
        assert_eq!(value(SizeClass::Medium), 3);
        assert_eq!(value(SizeClass::Expanded), 3);
    }

    #[test]
    fn zero_scale_is_treated_as_one() {
        let vp = Viewport::from_physical(Size::new(100.0, 50.0), 0.0);
        assert_eq!(vp.scale, 1.0);
        assert_eq!(vp.size, Size::new(100.0, 50.0));
    }
}
