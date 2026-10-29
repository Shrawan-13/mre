//! Core primitives shared by every layer of MRE.
//!
//! This crate has **no dependencies** and contains **no `unsafe`**. Everything
//! here is on the hot path, so the design constraints are:
//!
//! * Fixed, small, `Copy` value types (ids, rects, colours).
//! * One arena, zero `Rc`/`RefCell`, no pointer chasing.
//! * No allocation in any hot-path method.

pub mod arena;
pub mod color;
pub mod geom;
pub mod id;
pub mod viewport;

pub use arena::Arena;
pub use color::Rgba8;
pub use geom::{Point, Rect, Size};
pub use id::Id;
pub use viewport::{Breakpoints, Insets, Orientation, SizeClass, Viewport};

/// Bytes in a typical cache line on the targets we care about.
///
/// Used by tests to assert structural budgets (an `mre_scene` node is exactly
/// this size). Kept here so the number has exactly one definition.
pub const CACHE_LINE: usize = 64;
