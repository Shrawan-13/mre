//! Tests for the counting allocator itself.
//!
//! In its own binary, with one test, because the counters are process-global:
//! two tests enabling counting at the same time would see each other's
//! allocations and the gates would become flaky. Flaky gates get deleted.

use mre_perf::alloc::{self, CountingAllocator, Snapshot};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

fn assert_unchanged(label: &str, before: Snapshot, after: Snapshot) {
    assert_eq!(after.allocs, before.allocs, "{label}: allocation counted");
    assert_eq!(
        after.reallocs, before.reallocs,
        "{label}: reallocation counted"
    );
}

#[test]
fn counting_allocator_behaves() {
    // Counts when enabled.
    alloc::enable();
    let before = alloc::snapshot();
    let v: Vec<u64> = Vec::with_capacity(64);
    std::hint::black_box(&v);
    let after = alloc::snapshot();
    assert!(after.allocs > before.allocs, "allocation was not observed");
    assert!(
        after.bytes_live >= before.bytes_live,
        "live bytes did not increase"
    );
    drop(v);

    // Live bytes return to baseline once the value is freed.
    let before_free = alloc::snapshot();
    {
        let w = vec![0u8; 4096];
        std::hint::black_box(&w);
    }
    let after_free = alloc::snapshot();
    assert_eq!(
        after_free.bytes_live, before_free.bytes_live,
        "live bytes did not return to baseline after free"
    );

    // Silent when disabled.
    alloc::disable();
    let before = alloc::snapshot();
    let w = vec![0u8; 4096];
    std::hint::black_box(&w);
    let after = alloc::snapshot();
    assert_unchanged("while disabled", before, after);
    drop(w);

    // Deltas are per-window, not cumulative.
    alloc::enable();
    let base = alloc::snapshot();
    let one = vec![0u8; 32];
    let mid = alloc::snapshot();
    let two = vec![0u8; 32];
    let end = alloc::snapshot();
    std::hint::black_box((&one, &two));
    assert!(base.allocs_after(&mid) >= 1);
    assert!(mid.allocs_after(&end) >= 1);
    assert!(base.events_after(&end) >= 2);
}
