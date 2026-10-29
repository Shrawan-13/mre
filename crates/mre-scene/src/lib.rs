//! Retained scene graph for MRE.
//!
//! The full pipeline this crate is growing into:
//!
//! ```text
//!   set_state -> layout -> paint -> (tessellate -> submit)
//!                 ^         ^
//!                 |         +-- dirty: PAINT / TEXT
//!                 +------------ dirty: LAYOUT
//! ```
//!
//! Today the layout and paint stages are represented by the reference table
//! workload in [`table`], which exists so the performance gates have something
//! real to measure from day one.

pub mod dirty;
pub mod display;
pub mod node;
pub mod scene;
pub mod table;

pub use dirty::Dirty;
pub use display::{Command, DisplayList};
pub use node::{Node, NodeKind};
pub use scene::Scene;
pub use table::{COLUMNS, HEADER_HEIGHT, OVERSCAN, ROW_HEIGHT, TableModel, TableView};
