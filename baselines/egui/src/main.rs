//! egui baseline adapter.
//!
//! Renders the reference scene (`perf/scenes/table.toml`) using egui's own
//! virtualisation API (`ScrollArea::show_rows`) and measures with the same
//! protocol as `mre-bench`: warm up, time each frame individually, report
//! percentiles.
//!
//! # Scope discrepancy — read this before comparing
//!
//! egui's measured window includes **text shaping and tessellation** (it turns
//! shapes into triangle meshes on the CPU). MRE's current measurement covers
//! **layout + paint + display-list build only**. egui is therefore doing
//! strictly *more* work per frame.
//!
//! This is disclosed in the `scope` field of the JSON output, and the comparison
//! table marks such rows as non-comparable. The point of this adapter right now
//! is to prove the harness and establish a reference point — not to claim a win.

use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const ROWS: usize = 50_000;
const COLUMNS: usize = 6;
const ROW_HEIGHT: f32 = 32.0;
const WIDTH: f32 = 1440.0;
const HEIGHT: f32 = 900.0;
const HEADER_HEIGHT: f32 = 40.0;
const WARMUP: usize = 60;

const SCOPE: &str =
    "layout+paint+tessellation (cpu; includes shaping+tessellation, MRE does not yet)";

#[derive(Clone, Copy)]
struct Scenario {
    id: &'static str,
    frames: usize,
    scroll_per_frame: f32,
}

const SCENARIOS: &[Scenario] = &[
    Scenario { id: "W1-idle", frames: 600, scroll_per_frame: 0.0 },
    Scenario { id: "W2-scroll", frames: 600, scroll_per_frame: 4.0 },
    Scenario { id: "W3-fling", frames: 600, scroll_per_frame: 40.0 },
];

#[derive(Debug)]
struct Run {
    p50: u64,
    p95: u64,
    p99: u64,
    p999: u64,
    max: u64,
    draw_calls: usize,
    vertices: usize,
}

fn main() {
    println!("egui baseline — table-50k — {ROWS} rows");
    println!("scope: {SCOPE}");
    println!();
    println!(
        "{:<12} {:>6} {:>10} {:>10} {:>10} {:>12} {:>5} {:>8}",
        "workload", "frames", "p50", "p95", "p99", "max", "prims", "verts"
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
            run.draw_calls,
            run.vertices
        );
        results.push((*scenario, run));
    }

    write_report(&results);
}

fn micros(ns: u64) -> String {
    format!("{:.2}µs", ns as f64 / 1000.0)
}

fn bench(scenario: Scenario) -> Run {
    let ctx = egui::Context::default();
    let max_scroll = (ROWS as f32 * ROW_HEIGHT - HEIGHT).max(0.0);
    let mut scroll = 0.0f32;
    let mut time = 0.0f64;

    // A shared accumulator so the closure-free loop below stays readable.
    let mut samples: Vec<u64> = Vec::with_capacity(scenario.frames);
    let mut draw_calls = 0usize;
    let mut vertices = 0usize;

    let total = WARMUP + scenario.frames;
    for frame in 0..total {
        scroll = (scroll + scenario.scroll_per_frame).clamp(0.0, max_scroll);
        time += 1.0 / 120.0;

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH, HEIGHT),
            )),
            time: Some(time),
            ..Default::default()
        };

        let start = Instant::now();

        let mut output = ctx.run_ui(input, |ui| {
            // Reserve the header band so the body starts where it does in MRE.
            ui.allocate_space(egui::vec2(ui.available_width(), HEADER_HEIGHT));

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .vertical_scroll_offset(scroll)
                .show_rows(ui, ROW_HEIGHT, ROWS, |ui, range| {
                    for row in range {
                        ui.horizontal(|ui| {
                            for col in 0..COLUMNS {
                                ui.monospace(format!("cell-{row}-{col}"));
                            }
                        });
                    }
                });
        });

        // Tessellation is part of egui's per-frame CPU cost, so it belongs in
        // the measured window.
        let shapes = std::mem::take(&mut output.shapes);
        let primitives = ctx.tessellate(shapes, output.pixels_per_point);
        let elapsed = start.elapsed().as_nanos() as u64;

        // egui panics if texture deltas are dropped unapplied; we are headless
        // and never upload anything.
        output.drop_without_applying_deltas();

        if frame >= WARMUP {
            samples.push(elapsed);
            draw_calls = primitives.len();
            // Counting vertices proves content was actually generated. A fast
            // frame that draws nothing is not a fast frame, it is a bug.
            vertices = primitives
                .iter()
                .map(|p| match &p.primitive {
                    egui::epaint::Primitive::Mesh(mesh) => mesh.vertices.len(),
                    egui::epaint::Primitive::Callback(_) => 0,
                })
                .sum();
        }
    }

    samples.sort_unstable();
    Run {
        p50: percentile(&samples, 0.50),
        p95: percentile(&samples, 0.95),
        p99: percentile(&samples, 0.99),
        p999: percentile(&samples, 0.999),
        max: *samples.last().unwrap_or(&0),
        draw_calls,
        vertices,
    }
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap_or_else(|_| PathBuf::from("."))
}

fn write_report(results: &[(Scenario, Run)]) {
    let dir = repo_root().join("perf/results");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {e}", dir.display());
        return;
    }

    let rows: Vec<_> = results
        .iter()
        .map(|(scenario, run)| {
            json!({
                "engine": "egui",
                "workload": scenario.id,
                "frames": scenario.frames,
                "p50_ns": run.p50,
                "p95_ns": run.p95,
                "p99_ns": run.p99,
                "p999_ns": run.p999,
                "max_ns": run.max,
                "draw_calls": run.draw_calls,
                "commands": run.draw_calls,
                "vertices": run.vertices,
                "allocs_per_frame": 0.0,
            })
        })
        .collect();

    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let report = json!({
        "engine": "egui",
        "engine_version": "0.37",
        "scene": "table-50k",
        "host": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
        "unix_seconds": stamp,
        "scope": SCOPE,
        "comparable": false,
        "noncomparable_reason": "egui includes shaping and tessellation in the measured window; MRE does not yet",
        "results": rows,
    });

    let path = dir.join(format!("egui-{stamp}.json"));
    match serde_json::to_string_pretty(&report) {
        Ok(text) => match fs::write(&path, text) {
            Ok(()) => println!("\nwrote {}", path.display()),
            Err(e) => eprintln!("cannot write {}: {e}", path.display()),
        },
        Err(e) => eprintln!("cannot serialise report: {e}"),
    }
}
