//! Gate: deterministic structural budgets.
//!
//! These have no timing in them, so they are safe to run in CI on shared
//! runners. They catch the regressions that are cheap to fix now and expensive
//! to fix later: a node that grows past a cache line, a pool that starts scaling
//! with the data set, a display list that quietly needs more draw calls.

use mre_core::CACHE_LINE;
use mre_perf::budget;
use mre_scene::{Command, Node};

#[test]
fn node_is_exactly_one_cache_line() {
    assert_eq!(
        core::mem::size_of::<Node>(),
        budget::memory::NODE_STRUCT_BYTES as usize,
        "a scene node must fit in one cache line"
    );
    assert_eq!(core::mem::size_of::<Node>(), CACHE_LINE);
}

#[test]
fn command_stays_small_enough_to_stream() {
    assert!(
        core::mem::size_of::<Command>() <= 32,
        "display list commands got fat ({} bytes); painting will become \
         memory-bandwidth bound",
        core::mem::size_of::<Command>()
    );
}

#[test]
fn arena_overhead_per_node_is_within_budget() {
    let view = mre_scene::TableView::new(mre_bench::ROWS, mre_bench::viewport());
    let per_node = view.scene_bytes() / view.node_count();
    assert!(
        per_node <= budget::memory::BYTES_PER_NODE as usize,
        "arena costs {per_node} bytes per node, budget is {}",
        budget::memory::BYTES_PER_NODE
    );
}

#[test]
fn pool_is_sized_by_the_viewport_not_the_data_set() {
    let small = mre_scene::TableView::new(1_000, mre_bench::viewport());
    let huge = mre_scene::TableView::new(1_000_000, mre_bench::viewport());

    assert_eq!(
        small.pool_len(),
        huge.pool_len(),
        "row pool must not scale with the number of rows"
    );
    assert!(
        huge.pool_len() < 40,
        "pool of {} nodes is not viewport-sized",
        huge.pool_len()
    );
}

#[test]
fn virtualised_scene_uses_constant_memory() {
    let view = mre_scene::TableView::new(mre_bench::ROWS, mre_bench::viewport());
    // 64 KiB for a 50k-row table. If this ever needs megabytes, virtualisation
    // has silently stopped working.
    assert!(
        view.scene_bytes() <= 64 * 1024,
        "scene uses {} bytes for {} rows",
        view.scene_bytes(),
        mre_bench::ROWS
    );
    assert!(view.scene_bytes() <= budget::memory::ARENA_BYTES as usize);
}

#[test]
fn draw_calls_stay_within_the_batching_budget() {
    let (mut view, mut list) = mre_bench::warm_view(10, 0.0);
    view.frame(4.0, &mut list);

    let draw_calls = list.batch_count();
    assert!(
        draw_calls <= budget::batching::MAX_DRAW_CALLS as usize,
        "{draw_calls} draw calls exceeds the budget of {}",
        budget::batching::MAX_DRAW_CALLS
    );
}
