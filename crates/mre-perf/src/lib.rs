//! Performance instrumentation.
//!
//! Three pieces, all deliberately boring:
//!
//! * [`budget`] — `perf/budget.toml` compiled into constants.
//! * [`alloc`] — a counting global allocator that makes "zero allocations per
//!   frame" a test assertion instead of a code-review opinion.
//! * [`stats`] — percentiles, because means hide the frames users actually feel.

// Unsafe is confined to [`alloc`], which must implement `GlobalAlloc`. Deny it
// everywhere else so it cannot spread into instrumentation code.
#![deny(unsafe_code)]

pub mod alloc;
pub mod budget;
pub mod stats;

pub use budget::{FRAME_BUDGET_NS, NODE_BUDGET_BYTES, WEAK_DEVICE_BUDGET_NS};
pub use budget::{TABLE, Value, get};
pub use stats::{Stats, Summary};
