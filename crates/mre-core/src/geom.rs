//! Geometry: `f32` everywhere, `Copy`, no allocation.

/// A 2D point in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal position, increasing rightwards.
    pub x: f32,
    /// Vertical position, increasing downwards.
    pub y: f32,
}

impl Point {
    /// The origin.
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    /// Creates a point.
    #[inline]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A 2D size in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Size {
    /// A zero size.
    pub const ZERO: Self = Self { w: 0.0, h: 0.0 };

    /// Creates a size.
    #[inline]
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }
}

/// An axis-aligned rectangle in logical pixels, origin top-left.
///
/// An **empty** rect (`w <= 0 || h <= 0`) is the identity element for
/// [`Rect::union`]. This is what makes damage-region accumulation work: start
/// from [`Rect::EMPTY`] and union everything that changed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width. A value `<= 0` makes the rect empty.
    pub w: f32,
    /// Height. A value `<= 0` makes the rect empty.
    pub h: f32,
}

impl Rect {
    /// The additive identity for [`Rect::union`].
    pub const EMPTY: Self = Self {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    /// Creates a rect from its corner and size.
    #[inline]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Creates a rect at the origin with the given size.
    #[inline]
    pub const fn from_size(size: Size) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            w: size.w,
            h: size.h,
        }
    }

    /// The right edge (`x + w`).
    #[inline]
    pub const fn right(self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge (`y + h`).
    #[inline]
    pub const fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// The rect's size.
    #[inline]
    pub const fn size(self) -> Size {
        Size {
            w: self.w,
            h: self.h,
        }
    }

    /// True when the rect has no area. Empty rects are never painted.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Smallest rect containing both operands.
    ///
    /// `EMPTY.union(r) == r`, which makes this safe to fold over a dirty set.
    #[inline]
    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Self {
            x,
            y,
            w: right - x,
            h: bottom - y,
        }
    }

    /// Overlap of two rects, or [`Rect::EMPTY`] when they are disjoint.
    #[inline]
    pub fn intersect(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            Self::EMPTY
        } else {
            Self {
                x,
                y,
                w: right - x,
                h: bottom - y,
            }
        }
    }

    /// True when `p` lies inside the half-open rect `[x, right) x [y, bottom)`.
    #[inline]
    pub fn contains(self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// True when the two rects overlap by any positive area.
    #[inline]
    pub fn intersects(self, other: Self) -> bool {
        !self.intersect(other).is_empty()
    }

    /// True when `self` fully contains `other`.
    #[inline]
    pub fn contains_rect(self, other: Self) -> bool {
        other.is_empty()
            || (other.x >= self.x
                && other.y >= self.y
                && other.right() <= self.right()
                && other.bottom() <= self.bottom())
    }

    /// Moves the rect by the given offset.
    #[inline]
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            ..self
        }
    }

    /// Shrinks the rect by `amount` on every side, clamped at zero area.
    #[inline]
    pub fn inset(self, amount: f32) -> Self {
        Self {
            x: self.x + amount,
            y: self.y + amount,
            w: (self.w - amount * 2.0).max(0.0),
            h: (self.h - amount * 2.0).max(0.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_union_identity() {
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(Rect::EMPTY.union(r), r);
        assert_eq!(r.union(Rect::EMPTY), r);
    }

    #[test]
    fn union_is_bounding_box() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 5.0, 10.0, 10.0);
        let u = a.union(b);
        assert_eq!((u.x, u.y, u.w, u.h), (0.0, 0.0, 30.0, 15.0));
    }

    #[test]
    fn disjoint_intersect_is_empty() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(50.0, 50.0, 10.0, 10.0);
        assert!(a.intersect(b).is_empty());
        assert!(!a.intersects(b));
    }

    #[test]
    fn damaged_region_accumulates() {
        let mut damage = Rect::EMPTY;
        damage = damage.union(Rect::new(0.0, 0.0, 100.0, 10.0));
        damage = damage.union(Rect::new(0.0, 50.0, 100.0, 10.0));
        assert_eq!(damage, Rect::new(0.0, 0.0, 100.0, 60.0));
    }

    #[test]
    fn containment_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(9.999, 9.999)));
        assert!(
            !r.contains(Point::new(10.0, 5.0)),
            "right edge is exclusive"
        );
    }

    #[test]
    fn inset_clamps_to_empty() {
        let r = Rect::new(0.0, 0.0, 4.0, 4.0).inset(5.0);
        assert!(r.is_empty());
    }
}
