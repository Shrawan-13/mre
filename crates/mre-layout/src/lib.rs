//! Layout for MRE: flex, grid, stack and absolute positioning.
//!
//! The engine is **measure then arrange**, with a memoised measure pass keyed on
//! quantised constraints. That cache is what makes a resize cheap: only the
//! subtrees whose available space actually changed are re-measured.
//!
//! Responsiveness is structural, not special-cased. A rotation is a new
//! [`mre_core::Viewport`]; the caller lays out again and every percentage,
//! breakpoint and `Fr` track resolves against the new space.
//!
//! ```
//! use mre_layout::{px, Engine, Style, Tree};
//! use mre_core::{Rect, Size};
//!
//! struct NoText;
//! impl mre_layout::TextMetrics for NoText {
//!     fn measure(&mut self, _: mre_layout::TextId, _: f32) -> Size {
//!         Size::ZERO
//!     }
//! }
//!
//! let mut tree = Tree::new(Style::column().padding(mre_layout::Edges::all(8.0)));
//! let root = tree.root();
//! let child = tree.add_child(root, Style::default().height(px(40.0)));
//!
//! let mut engine = Engine::new();
//! let layout = engine.layout(&tree, Rect::new(0.0, 0.0, 320.0, 240.0), &mut NoText);
//!
//! assert_eq!(layout.rect(child).y, 8.0);
//! assert_eq!(layout.size(), Size::new(320.0, 240.0));
//! ```

pub mod engine;
pub mod style;
pub mod tree;

pub use engine::{Constraints, Engine, Layout, Stats, TextMetrics};
pub use style::{
    Align, Axis, Dim, Direction, Display, Edges, Justify, Position, Style, Track, pct, px,
};
pub use tree::{Content, LayoutNode, NodeId, TextId, Tree};
