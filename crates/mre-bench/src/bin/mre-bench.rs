//! Benchmark runner.
//!
//! ```text
//! cargo run --release -p mre-bench                 # time the reference scene
//! cargo run --release -p mre-bench -- --count-allocs
//! cargo run --release -p mre-bench -- compare      # table vs. baselines
//! ```

use mre_bench::{Measurement, SCENARIOS, cold_start, measure};
use mre_perf::alloc::CountingAllocator;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "compare") {
        compare();
        return;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("usage: mre-bench [--count-allocs] [--strict] [compare]");
        println!();
        println!("  --count-allocs  enable the counting allocator (slower, exact)");
        println!("  --strict        exit non-zero if any workload violates the contract");
        return;
    }

    if args.iter().any(|arg| arg == "--count-allocs") {
        mre_perf::alloc::enable();
    }

    let strict = args.iter().any(|arg| arg == "--strict");
    run(strict);
}

fn run(strict: bool) {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let host = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string());
    let cpus = thread::available_parallelism().map_or(0, |n| n.get());

    println!("MRE — table-50k — {} rows", mre_bench::ROWS);
    println!(
        "host: {host}  cpus: {cpus}  mre: {}",
        env!("CARGO_PKG_VERSION")
    );
    println!("scope: layout + paint + display-list build on the CPU.");
    println!("       No GPU submit, no shaping, no rasterisation. Compare only with");
    println!("       engines reporting the same scope — see docs/COMPARISON.md.");
    println!();

    let header = format!(
        "{:<12} {:>6} {:>10} {:>10} {:>10} {:>10} {:>10} {:>11} {:>5} {:>6} {:>13}",
        "workload",
        "frames",
        "p50",
        "p95",
        "p99",
        "p999",
        "max",
        "p99/budget",
        "draw",
        "cmds",
        "allocs/frame"
    );
    println!("{header}");
    println!("{}", "-".repeat(header.len()));

    let mut results = Vec::new();
    for scenario in SCENARIOS {
        let m = measure(*scenario);
        print_row(&m);
        results.push(m);
    }

    // Cold start is a separate axis: dominated by allocation and setup, not by
    // steady-state throughput.
    let cold_ns = cold_start();
    println!();
    println!(
        "cold start (construct + first frame): {:.1} µs  (budget {} µs)",
        cold_ns as f64 / 1000.0,
        mre_perf::budget::startup::FIRST_FRAME_NS as f64 / 1000.0
    );

    println!();
    let failures: Vec<&Measurement> = results.iter().filter(|m| !m.within_budget()).collect();
    if failures.is_empty() {
        println!("verdict: all workloads inside the performance contract");
    } else {
        println!(
            "verdict: {} workload(s) violated the contract",
            failures.len()
        );
        for m in &failures {
            println!(
                "  - {} p99 {:.2} µs vs budget {:.2} µs",
                m.workload,
                m.summary.p99_ns as f64 / 1000.0,
                m.budget_ns as f64 / 1000.0
            );
        }
    }

    write_report(&results, cold_ns, &host, cpus, started);

    if strict && !failures.is_empty() {
        std::process::exit(1);
    }
}

fn print_row(m: &Measurement) {
    let s = &m.summary;
    println!(
        "{:<12} {:>6} {:>10} {:>10} {:>10} {:>10} {:>10} {:>11} {:>5} {:>6} {:>13.2}",
        m.workload,
        m.frames,
        micros(s.p50_ns),
        micros(s.p95_ns),
        micros(s.p99_ns),
        micros(s.p999_ns),
        micros(s.max_ns),
        format!("{:.2}%", s.p99_ratio(m.budget_ns) * 100.0),
        m.draw_calls,
        m.commands,
        m.allocs_per_frame()
    );
}

fn micros(ns: u64) -> String {
    format!("{:.2}µs", ns as f64 / 1000.0)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn write_report(results: &[Measurement], cold_ns: u64, host: &str, cpus: usize, stamp: u64) {
    let dir = repo_root().join("perf/results");
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {e}", dir.display());
        return;
    }

    let rows: Vec<Value> = results
        .iter()
        .map(|m| {
            json!({
                "engine": m.engine,
                "scene": m.scene,
                "workload": m.workload,
                "frames": m.frames,
                "p50_ns": m.summary.p50_ns,
                "p95_ns": m.summary.p95_ns,
                "p99_ns": m.summary.p99_ns,
                "p999_ns": m.summary.p999_ns,
                "max_ns": m.summary.max_ns,
                "mean_ns": m.summary.mean_ns,
                "budget_ns": m.budget_ns,
                "draw_calls": m.draw_calls,
                "commands": m.commands,
                "nodes": m.nodes,
                "scene_bytes": m.scene_bytes,
                "allocs_per_frame": m.allocs_per_frame(),
                "bytes_peak": m.bytes_peak,
            })
        })
        .collect();

    let report = json!({
        "engine": "mre",
        "engine_version": env!("CARGO_PKG_VERSION"),
        "scene": "table-50k",
        "rows": mre_bench::ROWS,
        "host": host,
        "cpus": cpus,
        "unix_seconds": stamp,
        "cold_start_ns": cold_ns,
        "scope": "layout+paint+display_list (cpu, no gpu submit, no shaping, no rasterisation)",
        "results": rows,
    });

    let path = dir.join(format!("mre-{stamp}.json"));
    match serde_json::to_string_pretty(&report) {
        Ok(text) => match fs::write(&path, text) {
            Ok(()) => println!("\nwrote {}", path.display()),
            Err(e) => eprintln!("cannot write {}: {e}", path.display()),
        },
        Err(e) => eprintln!("cannot serialise report: {e}"),
    }
}

/// Merges every report in `perf/results` into one comparison table.
///
/// Reports are deduplicated by `(engine, workload)`, keeping the newest run, and
/// every row carries its measurement scope. Rows whose scope differs from MRE's
/// are marked with `*` and called out below the table: comparing a window that
/// includes text shaping against one that does not is exactly the dishonesty
/// this harness exists to prevent.
fn compare() {
    use std::collections::HashMap;

    #[derive(Debug)]
    struct Row {
        engine: String,
        workload: String,
        p50: u64,
        p99: u64,
        draw_calls: usize,
        allocs: f64,
        comparable: bool,
    }

    let dir = repo_root().join("perf/results");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("cannot read {}: {e}", dir.display());
            eprintln!("run a benchmark first: cargo run --release -p mre-bench");
            return;
        }
    };

    let mut latest: HashMap<(String, String), (u64, Row)> = HashMap::new();
    let mut scopes: HashMap<String, String> = HashMap::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(report) = serde_json::from_str::<Value>(&text) else {
            continue;
        };

        let engine = report
            .get("engine")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        let scope = report
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("unspecified")
            .to_string();
        let comparable = report
            .get("comparable")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let stamp = report
            .get("unix_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        scopes.insert(engine.clone(), scope);

        let Some(results) = report.get("results").and_then(Value::as_array) else {
            continue;
        };
        for row in results {
            let get_str = |k: &str| {
                row.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or("?")
                    .to_string()
            };
            let get_u64 = |k: &str| row.get(k).and_then(Value::as_u64).unwrap_or(0);

            let workload = get_str("workload");
            let candidate = Row {
                engine: engine.clone(),
                workload: workload.clone(),
                p50: get_u64("p50_ns"),
                p99: get_u64("p99_ns"),
                draw_calls: row.get("draw_calls").and_then(Value::as_u64).unwrap_or(0) as usize,
                allocs: row
                    .get("allocs_per_frame")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0),
                comparable,
            };

            let key = (engine.clone(), workload);
            match latest.get(&key) {
                Some((existing, _)) if *existing >= stamp => {}
                _ => {
                    latest.insert(key, (stamp, candidate));
                }
            }
        }
    }

    if latest.is_empty() {
        println!("no results found in {}", dir.display());
        return;
    }

    let mut rows: Vec<Row> = latest.into_values().map(|(_, row)| row).collect();
    rows.sort_by(|a, b| a.workload.cmp(&b.workload).then(a.p99.cmp(&b.p99)));

    println!("| workload | engine | p50 | p99 | vs best p99 | draw calls | allocs/frame |");
    println!("|---|---|---|---|---|---|---|");

    let mut current = "";
    let mut best_p99 = 0u64;
    for row in &rows {
        if row.workload != current {
            current = &row.workload;
            best_p99 = row.p99;
        } else if row.p99 < best_p99 {
            best_p99 = row.p99;
        }
        let ratio = if best_p99 == 0 {
            1.0
        } else {
            row.p99 as f64 / best_p99 as f64
        };
        let marker = if row.comparable { "" } else { " *" };
        println!(
            "| {} | {}{} | {:.2} µs | {:.2} µs | {:.2}x | {} | {:.2} |",
            row.workload,
            row.engine,
            marker,
            row.p50 as f64 / 1000.0,
            row.p99 as f64 / 1000.0,
            ratio,
            row.draw_calls,
            row.allocs
        );
    }

    println!();
    println!("`vs best p99` is relative to the fastest engine for that workload (1.00x = tie).");
    println!("`*` marks a row measured over a different scope — not directly comparable.");
    println!();
    println!("measurement scopes:");
    let mut sorted: Vec<_> = scopes.into_iter().collect();
    sorted.sort();
    for (engine, scope) in sorted {
        println!("  {engine}: {scope}");
    }
    println!();
    println!("See docs/COMPARISON.md for the fairness protocol before quoting any of this.");
}
