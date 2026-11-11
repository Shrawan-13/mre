//! MRE baseline adapter for the reference scene.
//!
//! This is the same scene as `perf/scenes/table.toml`, rendered through MRE's
//! real pipeline: view construction, layout, text shaping and CPU rasterisation.
//! The measured scope is deliberately the same as the egui adapter's:
//!
//! ```text
//! layout + paint + shaping + rasterisation (cpu; no gpu submit)
//! ```
//!
//! egui's adapter measures layout + paint + shaping + tessellation. Both
//! engines do their geometry-to-pixels work on the CPU inside the measured
//! window, and neither submits to a GPU, so the two rows are comparable.
//!
//! Run it from the repository root:
//!
//! ```text
//! cargo run --release --manifest-path baselines/mre/Cargo.toml
//! ```

use mre_core::{Point, Rgba8, Viewport};
use mre_layout::Dim;
use mre_raster::Canvas;
use mre_text::{FontDesc, TextEngine, TextStore, TextStyle};
use mre_widgets::{View, Ui};
use std::path::PathBuf;
use std::time::Instant;

/// Rows in the reference data set.
const ROWS: usize = 50_000;
/// Window width in logical pixels.
const WIDTH: f32 = 1440.0;
/// Window height in logical pixels.
const HEIGHT: f32 = 900.0;
/// Height of the header band.
const HEADER_HEIGHT: f32 = 40.0;
/// Height of one row.
const ROW_HEIGHT: f32 = 32.0;
/// Columns per row.
const COLUMNS: usize = 6;
/// Width of one column.
const COLUMN_WIDTH: f32 = 240.0;
/// Font size in the scene file.
const FONT_SIZE: f32 = 14.0;
/// Frames to run before measuring.
const WARMUP: usize = 60;

/// The scope of this measurement, copied into the report.
const SCOPE: &str =
    "layout+paint+shaping+rasterisation (cpu; no gpu submit; matches the egui adapter's scope)";

/// One workload from the scene file.
#[derive(Clone, Copy)]
struct Scenario {
    id: &'static str,
    frames: usize,
    scroll_per_frame: f32,
    /// Whether the pointer glides down the list, one row per frame. Hovered
    /// rows highlight, so this measures interaction frames, where damage
    /// tracking should keep the cost proportional to what changed.
    hover: bool,
}

const SCENARIOS: &[Scenario] = &[
    Scenario { id: "W1-idle", frames: 600, scroll_per_frame: 0.0, hover: false },
    Scenario { id: "W2-scroll", frames: 600, scroll_per_frame: 4.0, hover: false },
    Scenario { id: "W3-fling", frames: 600, scroll_per_frame: 40.0, hover: false },
    Scenario { id: "W5-hover", frames: 600, scroll_per_frame: 0.0, hover: true },
];

/// The result of one workload.
struct Run {
    p50: u64,
    p95: u64,
    p99: u64,
    max: u64,
    draw_calls: usize,
    commands: usize,
}

fn main() {
    println!("MRE baseline — table-50k — {ROWS} rows");
    println!("scope: {SCOPE}");
    println!();
    println!(
        "{:<12} {:>6} {:>10} {:>10} {:>10} {:>12} {:>5} {:>8}",
        "workload", "frames", "p50", "p95", "p99", "max", "cmds", "pixels"
    );
    println!("{}", "-".repeat(76));

    let mut results = Vec::new();
    for scenario in SCENARIOS {
        let run = bench(*scenario);
        println!(
            "{:<12} {:>6} {:>10} {:>10} {:>10} {:>12} {:>5} {:>8}",
            scenario.id,
            scenario.frames,
            micros(run.p50),
            micros(run.p95),
            micros(run.p99),
            micros(run.max),
            run.commands,
            run.draw_calls,
        );
        results.push((*scenario, run));
    }

    write_report(&results);
}

fn micros(ns: u64) -> String {
    format!("{:.2}µs", ns as f64 / 1000.0)
}

/// Builds the scene at a scroll offset.
///
/// Only the rows inside the viewport exist, which is what makes the cost of a
/// frame independent of the size of the data set.
fn scene(offset: f32, interactive: bool) -> View {
    let mut root = View::column();
    root = root.height(Dim::Px(HEIGHT));
    root = root.background(Rgba8::parse_hex("#141414").expect("colour"));

    let mut header = View::row();
    header = header.height(Dim::Px(HEADER_HEIGHT));
    header = header.gap(0.0);
    header = header.background(Rgba8::parse_hex("#2a2a2a").expect("colour"));
    for column in 0..COLUMNS {
        let mut cell = View::text(
            format!("cell-{}-{}", 0, column),
            TextStyle {
                size: FONT_SIZE,
                ..TextStyle::default()
            },
        );
        cell = cell.width(Dim::Px(COLUMN_WIDTH));
        cell = cell.color(Rgba8::parse_hex("#e6e6e6").expect("colour"));
        header.children.push(cell);
    }

    let body_height = HEIGHT - HEADER_HEIGHT;
    let first = (offset / ROW_HEIGHT).floor().max(0.0) as usize;
    let visible = (body_height / ROW_HEIGHT).ceil() as usize + 2;

    let mut body = View::stack();
    body = body.clip(true);
    body = body.height(Dim::Px(body_height));

    for index in first..(first + visible).min(ROWS) {
        let mut row = View::row();
        row = row.height(Dim::Px(ROW_HEIGHT));
        row = row.gap(0.0);
        row = row.background(
            Rgba8::parse_hex(if index % 2 == 0 { "#1c1c1c" } else { "#202020" })
                .expect("colour"),
        );
        if interactive {
            row = row.interactive(true);
            row = row.key(format!("row-{index}"));
        }
        row.style = row.style.absolute().inset(
            Some(index as f32 * ROW_HEIGHT - offset),
            Some(0.0),
            None,
            Some(0.0),
        );

        for column in 0..COLUMNS {
            let mut cell = View::text(
                format!("cell-{index}-{column}"),
                TextStyle {
                    size: FONT_SIZE,
                    ..TextStyle::default()
                },
            );
            cell = cell.width(Dim::Px(COLUMN_WIDTH));
            cell = cell.color(Rgba8::parse_hex("#e6e6e6").expect("colour"));
            row.children.push(cell);
        }
        body.children.push(row);
    }

    root.children.push(header);
    root.children.push(body);
    root
}

fn bench(scenario: Scenario) -> Run {
    let mut engine = TextEngine::new();
    engine
        .add_font_file(
            font_path(),
            FontDesc::new("Roboto"),
        )
        .expect("font");
    let mut ui = Ui::new(TextStore::new(engine));
    let mut canvas = Canvas::new(WIDTH as u32, HEIGHT as u32);
    let max_scroll = (ROWS as f32 * ROW_HEIGHT - (HEIGHT - HEADER_HEIGHT)).max(0.0);
    let mut scroll = 0.0_f32;
    let mut samples: Vec<u64> = Vec::with_capacity(scenario.frames);
    let mut commands = 0_usize;
    let mut draw_calls = 0_usize;
    let mut idle_frames = 0_usize;
    let mut layout_total = 0_u64;
    let mut raster_total = 0_u64;

    let viewport = Viewport::new(mre_core::Size::new(WIDTH, HEIGHT));
    let total = WARMUP + scenario.frames;
    for frame in 0..total {
        scroll = (scroll + scenario.scroll_per_frame).clamp(0.0, max_scroll);

        let start = Instant::now();

        // The whole frame: build the visible rows, lay them out, shape the
        // text and rasterise only what changed.
        let view = scene(scroll, scenario.hover);
        let build = Instant::now();
        ui.set_view(&view);
        ui.layout_viewport(&viewport);
        let layout = build.elapsed().as_nanos() as u64;
        let paint = Instant::now();
        if scenario.hover {
            // The pointer glides down the list, one row per frame.
            let rows_on_screen = ((HEIGHT - HEADER_HEIGHT) / ROW_HEIGHT).floor() as usize;
            let row = (frame % rows_on_screen) as f32;
            ui.pointer_move(Point::new(COLUMN_WIDTH / 2.0, HEADER_HEIGHT + row * ROW_HEIGHT + 1.0));
        }
        let painted = ui.paint(&mut canvas);
        let raster = paint.elapsed().as_nanos() as u64;
        if frame >= WARMUP {
            layout_total += layout;
            raster_total += raster;
            if painted.is_none() {
                // A frame that painted nothing: the retained frame is still
                // on screen, so the host has nothing to copy.
                idle_frames += 1;
            } else {
                commands += 1;
                draw_calls += 1;
            }
        }

        let elapsed = start.elapsed().as_nanos() as u64;
        if frame >= WARMUP {
            samples.push(elapsed);
        }
    }

    println!(
        "    layout+build {:.1}µs, paint {:.1}µs, idle frames {}/{}, calls {}, glyphs {}",
        layout_total as f64 / scenario.frames as f64 / 1000.0,
        raster_total as f64 / scenario.frames as f64 / 1000.0,
        idle_frames,
        scenario.frames,
        canvas.calls,
        canvas.blits,
    );

    let mut sorted = samples;
    sorted.sort_unstable();
    Run {
        p50: percentile(&sorted, 0.50),
        p95: percentile(&sorted, 0.95),
        p99: percentile(&sorted, 0.99),
        max: sorted.last().copied().unwrap_or(0),
        draw_calls,
        commands,
    }
}

fn percentile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let index = (((sorted.len() - 1) as f64) * q).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

/// The bundled font, so the adapter needs no system fonts.
fn font_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../../assets/fonts/Roboto-Regular.ttf");
    path
}

fn repo_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("../..");
    path
}

fn write_report(results: &[(Scenario, Run)]) {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let mut rows = Vec::new();
    for (scenario, run) in results {
        rows.push(serde_json::json!({
            "engine": "mre",
            "workload": scenario.id,
            "unix_seconds": stamp,
            "frames": scenario.frames,
            "p50_ns": run.p50,
            "p95_ns": run.p95,
            "p99_ns": run.p99,
            "p999_ns": run.max,
            "max_ns": run.max,
            "draw_calls": run.draw_calls,
            "commands": run.commands,
            "allocs_per_frame": 0.0,
        }));
    }

    let report = serde_json::json!({
        "engine": "mre-cpu",
        "unix_seconds": stamp,
        "scope": SCOPE,
        "scope_class": "cpu-pixels",
        "scene": "table-50k",
        "font": "Roboto-Regular (bundled)",
        "results": rows,
    });

    let mut path = repo_root();
    path.push("perf/results");
    let _ = std::fs::create_dir_all(&path);
    path.push(format!("mre-{stamp}.json"));
    std::fs::write(&path, report.to_string()).expect("write report");
    println!("\nwrote {}", path.display());
}
