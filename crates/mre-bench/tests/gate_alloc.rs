//! Gate: **no frame allocates.**
//!
//! Deliberately one test in one binary. The counting allocator is process-global
//! and the test harness runs tests in parallel, so two counting tests in the
//! same binary would observe each other's allocations and this gate would become
//! flaky — and a flaky gate gets muted, which is worse than no gate.
//!
//! The three windows below are the three ways a retained toolkit can silently
//! reintroduce allocation:
//!
//! * **idle** — a `Vec` rebuilt instead of cleared, a cache that rehashes.
//! * **scroll** — rows created per frame instead of recycled.
//! * **fling** — a damage list or glyph run cache that grows without bound.

use mre_perf::alloc::{self, CountingAllocator};
use mre_scene::{DisplayList, TableView};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

/// Runs `frames` frames and asserts the window performed zero allocation events.
fn assert_no_allocation(
    label: &str,
    view: &mut TableView,
    list: &mut DisplayList,
    scroll_per_frame: f32,
    frames: usize,
) {
    alloc::enable();
    let before = alloc::snapshot();
    for _ in 0..frames {
        view.frame(scroll_per_frame, list);
    }
    let after = alloc::snapshot();
    alloc::disable();

    let allocs = before.allocs_after(&after);
    let reallocs = before.reallocs_after(&after);
    assert_eq!(
        allocs + reallocs,
        0,
        "{label}: {frames} frames triggered {allocs} allocations and {reallocs} reallocations"
    );
}

#[test]
fn no_frame_allocates() {
    let (mut view, mut list) = mre_bench::warm_view(30, 0.0);

    // Sanity: a lying "idle" measurement is worse than no measurement.
    assert!(!list.is_empty(), "warm-up produced no display commands");

    assert_no_allocation("idle", &mut view, &mut list, 0.0, 1_000);
    assert_no_allocation("scroll", &mut view, &mut list, 4.0, 1_000);
    assert_no_allocation("fling", &mut view, &mut list, 40.0, 500);
    assert_no_allocation("reverse", &mut view, &mut list, -40.0, 500);

    // Deliberately allow one allocation so the harness itself is not the gate:
    // if this assert ever fires, the instrument is broken, not the engine.
    alloc::enable();
    let before = alloc::snapshot();
    let canary = vec![0u8; 8];
    let after = alloc::snapshot();
    alloc::disable();
    assert!(
        before.events_after(&after) >= 1,
        "allocator counter is not working"
    );
    drop(canary);
}
