//! The benchmark harness.
//!
//! One job: run the reference workload and report honest numbers. Every engine
//! in `baselines/` is measured by the same protocol so results are comparable.
//!
//! Protocol, applied to every engine:
//!
//! 1. Build the scene and warm the caches for `warmup` frames.
//! 2. Time `frames` frames individually with a monotonic clock.
//! 3. Report p50/p90/p95/p99/p999/max — never a mean alone.
//! 4. Count allocations, if the binary installed the counting allocator.
//! 5. Record draw calls and command counts so "fast" can be distinguished from
//!    "drawing less".

use mre_core::Rect;
use mre_perf::stats::{Stats, Summary};
use mre_perf::{FRAME_BUDGET_NS, alloc};
use mre_scene::{DisplayList, HEADER_HEIGHT, TableView};
use std::hint::black_box;
use std::time::Instant;

/// Rows in the reference data set.
pub const ROWS: usize = 50_000;
/// Window width in logical pixels.
pub const WINDOW_WIDTH: f32 = 1440.0;
/// Window height in logical pixels.
pub const WINDOW_HEIGHT: f32 = 900.0;
/// Display list capacity reserved during warm-up.
pub const DISPLAY_LIST_CAPACITY: usize = 4096;

/// The visible table body.
#[must_use]
pub fn viewport() -> Rect {
    Rect::new(
        0.0,
        HEADER_HEIGHT,
        WINDOW_WIDTH,
        WINDOW_HEIGHT - HEADER_HEIGHT,
    )
}

/// One benchmark definition.
#[derive(Clone, Copy, Debug)]
pub struct Scenario {
    /// Stable id, used as the cross-engine join key.
    pub id: &'static str,
    /// Number of timed frames.
    pub frames: usize,
    /// Pixels scrolled per frame.
    pub scroll_per_frame: f32,
}

/// The workloads from `perf/scenes/table.toml`.
pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "W1-idle",
        frames: 600,
        scroll_per_frame: 0.0,
    },
    Scenario {
        id: "W2-scroll",
        frames: 600,
        scroll_per_frame: 4.0,
    },
    Scenario {
        id: "W3-fling",
        frames: 600,
        scroll_per_frame: 40.0,
    },
];

/// The result of one scenario. This is the unit every engine must produce.
#[derive(Clone, Debug)]
pub struct Measurement {
    pub engine: String,
    pub scene: String,
    pub workload: String,
    pub frames: usize,
    pub summary: Summary,
    /// Allocation calls during the measured window.
    pub allocs: u64,
    /// Reallocation calls during the measured window.
    pub reallocs: u64,
    /// Bytes live at the end of the window.
    pub bytes_live: u64,
    /// High-water mark of live bytes.
    pub bytes_peak: u64,
    pub draw_calls: usize,
    pub commands: usize,
    pub nodes: usize,
    pub scene_bytes: usize,
    pub budget_ns: u64,
}

impl Measurement {
    /// Allocation events per frame during the measured window.
    #[must_use]
    pub fn allocs_per_frame(&self) -> f64 {
        if self.frames == 0 {
            0.0
        } else {
            (self.allocs + self.reallocs) as f64 / self.frames as f64
        }
    }

    /// `true` when the p99 fits inside the frame budget.
    #[must_use]
    pub fn within_budget(&self) -> bool {
        !self.summary.exceeds(
            self.budget_ns,
            mre_perf::budget::stability::P99_RATIO,
            mre_perf::budget::stability::P999_RATIO,
        )
    }
}

/// Builds a warmed-up view. Used by both the bench and the gates.
#[must_use]
pub fn warm_view(warmup: usize, scroll: f32) -> (TableView, DisplayList) {
    let mut view = TableView::new(ROWS, viewport());
    let mut list = DisplayList::with_capacity(DISPLAY_LIST_CAPACITY);
    for _ in 0..warmup {
        view.frame(scroll, &mut list);
    }
    (view, list)
}

/// Runs one scenario and reports it.
///
/// Allocation counting is only active when the calling binary installed
/// [`mre_perf::alloc::CountingAllocator`] *and* enabled it; otherwise the
/// allocation fields are zero and timing is unperturbed.
#[must_use]
pub fn measure(scenario: Scenario) -> Measurement {
    let mut stats = Stats::with_capacity(scenario.frames);
    let (mut view, mut list) = warm_view(60, scenario.scroll_per_frame);

    let counting = alloc::is_enabled();
    let before = if counting {
        Some(alloc::snapshot())
    } else {
        None
    };

    for _ in 0..scenario.frames {
        let start = Instant::now();
        view.frame(scenario.scroll_per_frame, &mut list);
        stats.record(start.elapsed().as_nanos() as u64);
    }

    let after = if counting {
        Some(alloc::snapshot())
    } else {
        None
    };

    let summary = stats.summary();
    let (allocs, reallocs, bytes_live, bytes_peak) = match (before, after) {
        (Some(b), Some(a)) => (
            b.allocs_after(&a),
            b.reallocs_after(&a),
            a.bytes_live,
            a.bytes_peak,
        ),
        _ => (0, 0, 0, 0),
    };

    let draw_calls = list.batch_count();
    let commands = list.len();
    let nodes = view.node_count();
    let scene_bytes = view.scene_bytes();

    black_box(&view);
    black_box(&list);

    Measurement {
        engine: "mre".to_string(),
        scene: "table-50k".to_string(),
        workload: scenario.id.to_string(),
        frames: scenario.frames,
        summary,
        allocs,
        reallocs,
        bytes_live,
        bytes_peak,
        draw_calls,
        commands,
        nodes,
        scene_bytes,
        budget_ns: FRAME_BUDGET_NS,
    }
}

/// Time from "construct the engine" to "first frame produced".
///
/// Reported separately from steady-state frames because cold start is dominated
/// by different things (allocation, font loading, pipeline creation) and is the
/// axis where a small engine can genuinely beat a large one.
#[must_use]
pub fn cold_start() -> u64 {
    let start = Instant::now();
    let mut view = TableView::new(ROWS, viewport());
    let mut list = DisplayList::with_capacity(DISPLAY_LIST_CAPACITY);
    view.frame(0.0, &mut list);
    black_box(&view);
    black_box(&list);
    start.elapsed().as_nanos() as u64
}
