//! A counting global allocator, for turning "zero allocations" into a test.
//!
//! This is the single most important perf gate in the repository. Everything
//! else — frame time, memory, jank — can be explained or excused. An allocation
//! in a steady-state frame cannot: it is an unbounded latency source that the
//! engine cannot reason about, and it is how GC-shaped stutter creeps back into
//! a non-GC language.
//!
//! # Usage
//!
//! ```no_run
//! use mre_perf::alloc::{self, CountingAllocator};
//!
//! #[global_allocator]
//! static ALLOC: CountingAllocator = CountingAllocator;
//!
//! let before = alloc::snapshot();
//! // ... hot loop ...
//! let after = alloc::snapshot();
//! assert_eq!(after.allocs - before.allocs, 0);
//! ```
//!
//! # Overhead
//!
//! Counting is opt-in via [`enable`]. When disabled the hooks branch on a
//! relaxed atomic load and then hand straight to the system allocator, so
//! timing runs can link the same allocator and stay honest. Even when enabled,
//! the cost is one uncontended atomic increment per allocation — measurable, but
//! only in nanosecond-scale loops, and never in the benchmark numbers we publish.
#![allow(unsafe_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::alloc::{GlobalAlloc, Layout, System};

static ENABLED: AtomicBool = AtomicBool::new(false);
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static DEALLOCS: AtomicU64 = AtomicU64::new(0);
static REALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES_LIVE: AtomicU64 = AtomicU64::new(0);
static BYTES_PEAK: AtomicU64 = AtomicU64::new(0);

/// Wraps the system allocator and, when enabled, tallies every operation.
///
/// Install it as `#[global_allocator]` in a test or benchmark binary. Never in
/// the shipped library.
pub struct CountingAllocator;

#[inline]
fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

#[inline]
fn bump_bytes(delta: u64) {
    let live = BYTES_LIVE.fetch_add(delta, Ordering::Relaxed) + delta;
    BYTES_PEAK.fetch_max(live, Ordering::Relaxed);
}

// SAFETY: every method forwards to `System`, which upholds the `GlobalAlloc`
// contract, and the statistics only observe size/pointer values. The counters
// never affect the returned pointer. `realloc` is only forwarded when the layout
// is valid, which is guaranteed by the `GlobalAlloc` contract.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: caller upholds the GlobalAlloc contract.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && enabled() {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            bump_bytes(layout.size() as u64);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if enabled() {
            DEALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES_LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        }
        // SAFETY: caller upholds the GlobalAlloc contract.
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: caller upholds the GlobalAlloc contract.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() && enabled() {
            REALLOCS.fetch_add(1, Ordering::Relaxed);
            // `fetch_add` wraps on underflow; use explicit widening instead so a
            // shrink cannot corrupt the peak.
            let old = layout.size() as u64;
            let new = new_size as u64;
            if new >= old {
                bump_bytes(new - old);
            } else {
                BYTES_LIVE.fetch_sub(old - new, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

/// A cheap, consistent-enough view of the counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Total allocation calls since process start.
    pub allocs: u64,
    /// Total deallocation calls since process start.
    pub deallocs: u64,
    /// Total reallocation calls since process start.
    pub reallocs: u64,
    /// Bytes currently live (allocated minus freed).
    pub bytes_live: u64,
    /// High-water mark of `bytes_live`.
    pub bytes_peak: u64,
}

impl Snapshot {
    /// Allocations that happened strictly after `self`.
    #[inline]
    #[must_use]
    pub fn allocs_after(&self, later: &Snapshot) -> u64 {
        later.allocs.saturating_sub(self.allocs)
    }

    /// Reallocations that happened strictly after `self`.
    #[inline]
    #[must_use]
    pub fn reallocs_after(&self, later: &Snapshot) -> u64 {
        later.reallocs.saturating_sub(self.reallocs)
    }

    /// Allocation *events* after `self`, counting realloc as an event.
    ///
    /// Both matter for the gate: a `Vec` growth is a realloc, not an alloc, and
    /// it is just as much a latency risk.
    #[inline]
    #[must_use]
    pub fn events_after(&self, later: &Snapshot) -> u64 {
        self.allocs_after(later) + self.reallocs_after(later)
    }
}

/// Begins counting. Idempotent.
pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
}

/// Stops counting. Idempotent.
pub fn disable() {
    ENABLED.store(false, Ordering::Relaxed);
}

/// True when counting is currently on.
#[inline]
#[must_use]
pub fn is_enabled() -> bool {
    enabled()
}

/// Zeroes every counter and the peak watermark.
///
/// The live-bytes counter is *not* zeroed, because it reflects real outstanding
/// allocations that will later be freed.
pub fn reset() {
    ALLOCS.store(0, Ordering::Relaxed);
    DEALLOCS.store(0, Ordering::Relaxed);
    REALLOCS.store(0, Ordering::Relaxed);
    BYTES_PEAK.store(BYTES_LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Reads the counters. Ordering is relaxed by design: this is instrumentation,
/// not synchronisation.
#[must_use]
pub fn snapshot() -> Snapshot {
    Snapshot {
        allocs: ALLOCS.load(Ordering::Relaxed),
        deallocs: DEALLOCS.load(Ordering::Relaxed),
        reallocs: REALLOCS.load(Ordering::Relaxed),
        bytes_live: BYTES_LIVE.load(Ordering::Relaxed),
        bytes_peak: BYTES_PEAK.load(Ordering::Relaxed),
    }
}
