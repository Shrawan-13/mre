//! End-to-end frame cost for the Lua path.
//!
//! Unlike `mre-bench`, which isolates the scene so numbers are comparable with
//! other engines, this measures what an app actually pays: Lua execution, view
//! conversion, layout, shaping, rasterisation and the copy into the canvas.
//!
//! ```text
//! cargo run --release -p mre-runtime --example bench -- examples/table.lua
//! ```
//!
//! Compare numbers only within one machine and one build profile. The scope is
//! printed with the table so a result cannot be quoted without it.

use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};
use std::time::Instant;

struct Summary {
    p50: u64,
    p95: u64,
    p99: u64,
    max: u64,
    mean: u64,
}

fn summary(mut samples: Vec<u64>) -> Summary {
    samples.sort_unstable();
    let at = |q: f64| -> u64 {
        if samples.is_empty() {
            return 0;
        }
        let index = ((samples.len() - 1) as f64 * q).round() as usize;
        samples[index.min(samples.len() - 1)]
    };
    let mean = if samples.is_empty() {
        0
    } else {
        samples.iter().sum::<u64>() / samples.len() as u64
    };
    Summary {
        p50: at(0.50),
        p95: at(0.95),
        p99: at(0.99),
        max: samples[samples.len().saturating_sub(1)],
        mean,
    }
}

fn load(path: &str, width: f32, height: f32) -> Result<App, Box<dyn std::error::Error>> {
    let source = std::fs::read_to_string(path)?;
    let mut engine = TextEngine::new();
    engine.add_font_file(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Roboto-Regular.ttf"
        ),
        FontDesc::new("Roboto"),
    )?;
    let mut app = App::with_text_store(&source, TextStore::new(engine))?;
    app.resize(Size::new(width, height), 1.0, Insets::ZERO);
    app.frame()?;
    app.paint();
    Ok(app)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/table.lua".to_string());
    let width = std::env::args()
        .nth(2)
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000.0);
    let height = std::env::args()
        .nth(3)
        .and_then(|value| value.parse().ok())
        .unwrap_or(760.0);

    println!("MRE runtime — end-to-end frame cost");
    println!("app: {path}  viewport: {width}x{height}");
    println!("scope: Lua execution + view conversion + layout + shaping + CPU raster");
    println!();

    // Idle: the frame function with nothing to do. A platform loop calls this,
    // sees `needs_frame() == false`, and does not paint.
    let mut app = load(&path, width, height)?;
    for _ in 0..30 {
        app.frame()?;
    }
    let mut idle = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let start = Instant::now();
        app.frame()?;
        idle.push(start.elapsed().as_nanos() as u64);
    }
    let idle = summary(idle);

    // Scroll: one wheel event, the rebuild it triggers, then the full paint.
    for _ in 0..30 {
        app.scroll(400.0, 400.0, 0.0, 120.0)?;
    }
    let mut scroll = Vec::with_capacity(300);
    for index in 0..300 {
        let dy = if index % 2 == 0 { -120.0 } else { 120.0 };
        let start = Instant::now();
        app.scroll(400.0, 400.0, 0.0, dy)?;
        app.paint();
        scroll.push(start.elapsed().as_nanos() as u64);
    }
    let scroll = summary(scroll);

    // Rebuild only: the Lua call, conversion and layout that one wheel event
    // triggers, without the rasteriser.
    let mut rebuild = Vec::with_capacity(300);
    for index in 0..300 {
        let dy = if index % 2 == 0 { -120.0 } else { 120.0 };
        let start = Instant::now();
        app.scroll(400.0, 400.0, 0.0, dy)?;
        rebuild.push(start.elapsed().as_nanos() as u64);
    }
    let rebuild = summary(rebuild);

    // Paint only: the cost of presenting an unchanged frame.
    let mut paint = Vec::with_capacity(200);
    for _ in 0..200 {
        let start = Instant::now();
        app.paint();
        paint.push(start.elapsed().as_nanos() as u64);
    }
    let paint = summary(paint);

    // Rotation / resize: a new viewport, then layout and paint.
    let mut resize = Vec::with_capacity(120);
    for index in 0..120 {
        let (w, h) = if index % 2 == 0 {
            (height, width)
        } else {
            (width, height)
        };
        let start = Instant::now();
        app.resize(Size::new(w, h), 1.0, Insets::ZERO);
        app.frame()?;
        app.paint();
        resize.push(start.elapsed().as_nanos() as u64);
    }
    let resize = summary(resize);

    println!(
        "{:<12} {:>8} {:>10} {:>10} {:>10} {:>10}",
        "workload", "samples", "p50", "p95", "p99", "max"
    );
    println!("{}", "-".repeat(64));
    for (name, samples, s) in [
        ("idle", 2000, &idle),
        ("scroll+paint", 300, &scroll),
        ("scroll+rebuild", 300, &rebuild),
        ("paint", 200, &paint),
        ("resize+paint", 120, &resize),
    ] {
        println!(
            "{name:<12} {samples:>8} {:>9.1}µs {:>9.1}µs {:>9.1}µs {:>9.1}µs",
            s.p50 as f64 / 1000.0,
            s.p95 as f64 / 1000.0,
            s.p99 as f64 / 1000.0,
            s.max as f64 / 1000.0
        );
    }
    let _ = idle.mean;

    // Typing into a form, which is the other interactive reference workload.
    if std::path::Path::new("examples/form.lua").exists() {
        let mut form = load("examples/form.lua", 420.0, 360.0)?;
        let rect = form.ui().rect_of_key("name").expect("name field");
        form.click(rect.x + 20.0, rect.y + rect.h / 2.0)?;
        let mut typing = Vec::with_capacity(300);
        for index in 0..300 {
            let character = if index % 7 == 0 { "\u{8}" } else { "a" };
            let start = Instant::now();
            if character == "\u{8}" {
                form.key("backspace", false)?;
            } else {
                form.insert_text(character)?;
            }
            form.paint();
            typing.push(start.elapsed().as_nanos() as u64);
        }
        let typing = summary(typing);
        println!(
            "{:<12} {:>8} {:>9.1}µs {:>9.1}µs {:>9.1}µs {:>9.1}µs",
            "keystroke",
            300,
            typing.p50 as f64 / 1000.0,
            typing.p95 as f64 / 1000.0,
            typing.p99 as f64 / 1000.0,
            typing.max as f64 / 1000.0
        );
    }

    Ok(())
}
