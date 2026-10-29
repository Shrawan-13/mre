//! Layout style: a small, deliberate subset of CSS.
//!
//! Everything an application layout needs is here; everything a document layout
//! needs is not. There is no float, no inline formatting, no table model.

/// A length that may depend on the available space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dim {
    /// Size to content.
    Auto,
    /// A fixed logical-pixel length.
    Px(f32),
    /// A percentage of the available space.
    Percent(f32),
}

impl Dim {
    /// Resolves against an available length.
    ///
    /// A percentage against an unbounded axis resolves to `None` rather than
    /// infinity, so a percentage width inside a scrolling row behaves like
    /// `Auto` instead of producing an unusable layout.
    #[must_use]
    pub fn resolve(self, available: f32) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Px(value) => Some(value.max(0.0)),
            Self::Percent(percent) => {
                if available.is_finite() {
                    Some((available * percent / 100.0).max(0.0))
                } else {
                    None
                }
            }
        }
    }

    /// Resolves, falling back to a default.
    #[must_use]
    pub fn unwrap_or(self, available: f32, fallback: f32) -> f32 {
        self.resolve(available).unwrap_or(fallback)
    }
}

/// A fixed length.
#[must_use]
pub const fn px(value: f32) -> Dim {
    Dim::Px(value)
}

/// A percentage length.
#[must_use]
pub const fn pct(value: f32) -> Dim {
    Dim::Percent(value)
}

/// Which axis a container lays out along.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Left to right.
    Horizontal,
    /// Top to bottom.
    Vertical,
}

/// Flex direction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Children left to right.
    #[default]
    Row,
    /// Children top to bottom.
    Column,
    /// Children right to left.
    RowReverse,
    /// Children bottom to top.
    ColumnReverse,
}

impl Direction {
    /// The axis this direction runs along.
    #[must_use]
    pub const fn axis(self) -> Axis {
        match self {
            Self::Row | Self::RowReverse => Axis::Horizontal,
            Self::Column | Self::ColumnReverse => Axis::Vertical,
        }
    }

    /// Whether children are placed in reverse order.
    #[must_use]
    pub const fn is_reverse(self) -> bool {
        matches!(self, Self::RowReverse | Self::ColumnReverse)
    }
}

/// How a node participates in layout.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Display {
    /// Takes no space and paints nothing.
    None,
    /// Flex container.
    #[default]
    Flex,
    /// Grid container.
    Grid,
    /// All children overlap, aligned by `align_items`/`justify_content`.
    Stack,
}

/// Cross-axis alignment of a child within its line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Align {
    /// Packed at the start of the cross axis.
    Start,
    /// Centred.
    Center,
    /// Packed at the end.
    End,
    /// Fills the cross axis. This is the default, because a child with an auto
    /// cross size inside a column should be as wide as the column.
    #[default]
    Stretch,
}

/// Main-axis distribution of free space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Justify {
    /// Packed at the start.
    #[default]
    Start,
    /// Centred.
    Center,
    /// Packed at the end.
    End,
    /// Equal space between items; none at the edges.
    SpaceBetween,
    /// Equal space around items, half at the edges.
    SpaceAround,
    /// Equal space everywhere.
    SpaceEvenly,
}

/// Whether a node is in normal flow or positioned against the container.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Position {
    /// Participates in the parent's flex/grid flow.
    #[default]
    Relative,
    /// Removed from flow, positioned by `inset`.
    Absolute,
}

/// Four edge lengths.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    /// Top edge.
    pub top: f32,
    /// Right edge.
    pub right: f32,
    /// Bottom edge.
    pub bottom: f32,
    /// Left edge.
    pub left: f32,
}

impl Edges {
    /// Zero on every edge.
    pub const ZERO: Self = Self {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };

    /// The same value on every edge.
    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Explicit edges.
    #[must_use]
    pub const fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    /// Symmetric horizontal / vertical edges.
    #[must_use]
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    /// Sum of the left and right edges.
    #[must_use]
    pub const fn horizontal(self) -> f32 {
        self.left + self.right
    }

    /// Sum of the top and bottom edges.
    #[must_use]
    pub const fn vertical(self) -> f32 {
        self.top + self.bottom
    }

    /// Sum along the main axis of a container.
    #[must_use]
    pub const fn main(self, horizontal: bool) -> f32 {
        if horizontal {
            self.horizontal()
        } else {
            self.vertical()
        }
    }

    /// Sum along the cross axis of a container.
    #[must_use]
    pub const fn cross(self, horizontal: bool) -> f32 {
        if horizontal {
            self.vertical()
        } else {
            self.horizontal()
        }
    }

    /// The leading edge along the main axis.
    #[must_use]
    pub const fn main_start(self, horizontal: bool) -> f32 {
        if horizontal { self.left } else { self.top }
    }

    /// The leading edge along the cross axis.
    #[must_use]
    pub const fn cross_start(self, horizontal: bool) -> f32 {
        if horizontal { self.top } else { self.left }
    }
}

/// A grid track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    /// A fixed logical-pixel size.
    Px(f32),
    /// A share of the leftover space.
    Fr(f32),
    /// The largest intrinsic size of the items in the track.
    Auto,
}

impl Track {
    /// A fixed track.
    #[must_use]
    pub const fn px(value: f32) -> Self {
        Self::Px(value)
    }

    /// A fractional track.
    #[must_use]
    pub const fn fr(value: f32) -> Self {
        Self::Fr(value)
    }
}

/// Layout properties of one node.
///
/// `Style` is `Clone` but not `Copy` because grid tracks are a `Vec`. Every
/// other field is a scalar, so the common case has no indirection.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// How children are arranged.
    pub display: Display,
    /// Flex direction.
    pub direction: Direction,

    /// Preferred width.
    pub width: Dim,
    /// Preferred height.
    pub height: Dim,
    /// Minimum width.
    pub min_width: Dim,
    /// Maximum width.
    pub max_width: Dim,
    /// Minimum height.
    pub min_height: Dim,
    /// Maximum height.
    pub max_height: Dim,
    /// `width / height`, applied when only one axis is definite.
    pub aspect_ratio: Option<f32>,

    /// Space inside the border box.
    pub padding: Edges,
    /// Space outside the border box, inside the parent.
    pub margin: Edges,
    /// Gap between flex children or grid tracks.
    pub gap: f32,

    /// Cross-axis alignment of children.
    pub align_items: Align,
    /// Main-axis distribution of free space.
    pub justify_content: Justify,

    /// How much of the free space this node absorbs.
    pub flex_grow: f32,
    /// How much this node shrinks when space is short.
    pub flex_shrink: f32,
    /// The starting main-axis size before grow/shrink.
    pub flex_basis: Dim,

    /// In-flow or absolutely positioned.
    pub position: Position,
    /// Offsets for absolutely positioned nodes. `None` means auto.
    pub inset: [Option<f32>; 4],

    /// Grid column tracks. Empty means one equal column per child.
    pub columns: Vec<Track>,
    /// Grid row tracks. Empty means auto rows.
    pub rows: Vec<Track>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            display: Display::Flex,
            direction: Direction::Column,
            width: Dim::Auto,
            height: Dim::Auto,
            min_width: Dim::Auto,
            max_width: Dim::Auto,
            min_height: Dim::Auto,
            max_height: Dim::Auto,
            aspect_ratio: None,
            padding: Edges::ZERO,
            margin: Edges::ZERO,
            gap: 0.0,
            align_items: Align::Stretch,
            justify_content: Justify::Start,
            flex_grow: 0.0,
            flex_shrink: 1.0,
            flex_basis: Dim::Auto,
            position: Position::Relative,
            inset: [None; 4],
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }
}

impl Style {
    /// A column container.
    #[must_use]
    pub fn column() -> Self {
        Self::default()
    }

    /// A row container.
    #[must_use]
    pub fn row() -> Self {
        Self {
            direction: Direction::Row,
            ..Self::default()
        }
    }

    /// A stack container: children overlap.
    #[must_use]
    pub fn stack() -> Self {
        Self {
            display: Display::Stack,
            ..Self::default()
        }
    }

    /// A grid container with the given column tracks.
    #[must_use]
    pub fn grid(columns: Vec<Track>) -> Self {
        Self {
            display: Display::Grid,
            columns,
            ..Self::default()
        }
    }

    /// Sets padding.
    #[must_use]
    pub fn padding(mut self, padding: Edges) -> Self {
        self.padding = padding;
        self
    }

    /// Sets margin.
    #[must_use]
    pub fn margin(mut self, margin: Edges) -> Self {
        self.margin = margin;
        self
    }

    /// Sets the flex direction.
    #[must_use]
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Sets the maximum width.
    #[must_use]
    pub fn max_width(mut self, value: Dim) -> Self {
        self.max_width = value;
        self
    }

    /// Sets the minimum width.
    #[must_use]
    pub fn min_width(mut self, value: Dim) -> Self {
        self.min_width = value;
        self
    }

    /// Sets the maximum height.
    #[must_use]
    pub fn max_height(mut self, value: Dim) -> Self {
        self.max_height = value;
        self
    }

    /// Sets the minimum height.
    #[must_use]
    pub fn min_height(mut self, value: Dim) -> Self {
        self.min_height = value;
        self
    }

    /// Sets the gap.
    #[must_use]
    pub fn gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Sets a fixed width.
    #[must_use]
    pub fn width(mut self, width: Dim) -> Self {
        self.width = width;
        self
    }

    /// Sets a fixed height.
    #[must_use]
    pub fn height(mut self, height: Dim) -> Self {
        self.height = height;
        self
    }

    /// Sets cross-axis alignment for children.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.align_items = align;
        self
    }

    /// Sets main-axis distribution for children.
    #[must_use]
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify_content = justify;
        self
    }

    /// Makes this node absorb free space.
    #[must_use]
    pub fn grow(mut self, factor: f32) -> Self {
        self.flex_grow = factor;
        self
    }

    /// Positions this node absolutely.
    #[must_use]
    pub fn absolute(mut self) -> Self {
        self.position = Position::Absolute;
        self
    }

    /// Sets the inset offsets for an absolutely positioned node.
    #[must_use]
    pub fn inset(
        mut self,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
        left: Option<f32>,
    ) -> Self {
        self.inset = [top, right, bottom, left];
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_need_a_bounded_axis() {
        assert_eq!(pct(50.0).resolve(800.0), Some(400.0));
        assert_eq!(pct(50.0).resolve(f32::INFINITY), None);
        assert_eq!(px(12.0).resolve(f32::INFINITY), Some(12.0));
        assert_eq!(Dim::Auto.resolve(100.0), None);
    }

    #[test]
    fn negative_lengths_clamp_to_zero() {
        assert_eq!(px(-10.0).resolve(100.0), Some(0.0));
        assert_eq!(pct(-5.0).resolve(100.0), Some(0.0));
    }

    #[test]
    fn direction_maps_to_axis_and_reversal() {
        assert_eq!(Direction::Row.axis(), Axis::Horizontal);
        assert_eq!(Direction::ColumnReverse.axis(), Axis::Vertical);
        assert!(Direction::ColumnReverse.is_reverse());
        assert!(!Direction::Row.is_reverse());
    }

    #[test]
    fn edges_swap_with_the_main_axis() {
        let e = Edges::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(e.main(true), 6.0);
        assert_eq!(e.cross(true), 4.0);
        assert_eq!(e.main(false), 4.0);
        assert_eq!(e.cross(false), 6.0);
        assert_eq!(e.main_start(true), 4.0);
        assert_eq!(e.main_start(false), 1.0);
    }
}
