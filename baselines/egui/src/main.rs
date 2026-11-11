//! egui baseline adapter.
//!
//! Renders the reference scene (`perf/scenes/table.toml`) using egui's own
//! virtualisation API (`ScrollArea::show_rows`) and measures with the same
//! protocol as `mre-bench`: warm up, time each frame individually, report
//! percentiles.
//!
//! # Scope — read this before comparing
//!
//! egui's measured window includes **text shaping and tessellation** (it turns
//! shapes into triangle meshes on the CPU) and stops before the GPU. The
//! `mre-cpu` adapter measures the same class of work on MRE's side: layout,
//! paint, shaping and CPU rasterisation, with no GPU submit.
//!
//! Both adapters therefore produce the same class of result: a frame that ends
//! in geometry, not in pixels on a screen. They are comparable with each
//! other, and not with the scene-graph-only `mre-bench` numbers, which stop
//! before shaping. The comparison table separates the two classes.

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
    "layout+paint+shaping+tessellation (cpu; no gpu submit; same scope class as the mre-cpu adapter)";

#[derive(Clone, Copy)]
struct Scenario {
    id: &'static str,
    frames: usize,
    scroll_per_frame: f32,
    /// Whether the pointer glides down the list, one row per frame, matching
    /// the MRE adapter's hover scenario.
    hover: bool,
}

const SCENARIOS: &[Scenario] = &[
    Scenario { id: "W1-idle", frames: 600, scroll_per_frame: 0.0, hover: false },
    Scenario { id: "W2-scroll", frames: 600, scroll_per_frame: 4.0, hover: false },
    Scenario { id: "W3-fling", frames: 600, scroll_per_frame: 40.0, hover: false },
    Scenario { id: "W5-hover", frames: 600, scroll_per_frame: 0.0, hover: true },
];

/// Row stripes and the hover highlight, from `perf/scenes/table.toml`.
const EVEN: egui::Color32 = egui::Color32::from_rgb(0x1c, 0x1c, 0x1c);
const ODD: egui::Color32 = egui::Color32::from_rgb(0x20, 0x20, 0x20);
const HOVERED: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2a, 0x2a);

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

        // The pointer glides down the list, one row per frame, matching the
        // MRE adapter's hover scenario.
        let rows_on_screen = ((HEIGHT - HEADER_HEIGHT) / ROW_HEIGHT).floor() as usize;
        let pointer_row = (frame % rows_on_screen) as f32;
        let pointer = egui::Pos2::new(120.0, HEADER_HEIGHT + pointer_row * ROW_HEIGHT + 1.0);
        let hovered_row = (scroll / ROW_HEIGHT).floor() as usize + (pointer_row as usize);
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH, HEIGHT),
            )),
            time: Some(time),
            ..Default::default()
        };
        if scenario.hover {
            input.events.push(egui::Event::PointerMoved(pointer));
        }

        let start = Instant::now();

        let mut output = ctx.run_ui(input, |ui| {
            // Reserve the header band so the body starts where it does in MRE.
            ui.allocate_space(egui::vec2(ui.available_width(), HEADER_HEIGHT));

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .vertical_scroll_offset(scroll)
                .show_rows(ui, ROW_HEIGHT, ROWS, |ui, range| {
                    for row in range {
                        // A hovered row highlights, the way a data table does.
                        let fill = if scenario.hover {
                            if row == hovered_row {
                                HOVERED
                            } else if row % 2 == 0 {
                                EVEN
                            } else {
                                ODD
                            }
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let top = ui.cursor().min.y;
                        if fill != egui::Color32::TRANSPARENT {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(
                                    egui::Pos2::new(0.0, top),
                                    egui::Pos2::new(ui.available_width(), top + ROW_HEIGHT),
                                ),
                                0.0,
                                fill,
                            );
                        }
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
        "scope_class": "cpu-pixels",
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
