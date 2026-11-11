//! Warm rebuild cost for a view tree.
//!
//! ```text
//! MRE_PROFILE=1 cargo run --release -p mre-runtime --example profile -- examples/table.lua
//! ```

use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/table.lua".to_string());
    let source = std::fs::read_to_string(&path)?;
    let mut engine = TextEngine::new();
    engine.add_font_file(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Roboto-Regular.ttf"
        ),
        FontDesc::new("Roboto"),
    )?;
    let mut app = App::with_text_store(&source, TextStore::new(engine))?;
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame()?;
    app.paint();

    let mut rebuild = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        app.refresh()?;
        rebuild.push(start.elapsed().as_nanos() as u64);
    }
    rebuild.sort_unstable();

    let mut paint = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        app.paint();
        paint.push(start.elapsed().as_nanos() as u64);
    }
    paint.sort_unstable();

    println!(
        "{path}: nodes={} rebuild p50={:.3}ms p99={:.3}ms  paint p50={:.3}ms p99={:.3}ms",
        app.ui().node_count(),
        rebuild[50] as f64 / 1.0e6,
        rebuild[99] as f64 / 1.0e6,
        paint[50] as f64 / 1.0e6,
        paint[99] as f64 / 1.0e6,
    );
    Ok(())
}
