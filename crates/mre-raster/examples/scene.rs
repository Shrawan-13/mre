//! A micro-benchmark of the reference scene's raster cost.
use mre_core::{Point, Rect, Rgba8};
use mre_raster::{Canvas, Paint};
use mre_text::{FontDesc, TextEngine, TextStyle};
use std::time::Instant;

fn main() {
    let mut engine = TextEngine::new();
    engine
        .add_font_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/Roboto-Regular.ttf"
            ),
            FontDesc::new("Roboto"),
        )
        .unwrap();
    let style = TextStyle {
        size: 14.0,
        ..TextStyle::default()
    };
    let mut canvas = Canvas::new(1440, 900);
    let n = 200;

    let start = Instant::now();
    for _ in 0..n {
        canvas.clear(Rgba8::parse_hex("#141414").unwrap());
    }
    println!(
        "clear 1440x900:      {:>8.1}µs",
        start.elapsed().as_nanos() as f64 / n as f64 / 1000.0
    );

    let start = Instant::now();
    for _i in 0..n {
        for row in 0..27 {
            let y = 40.0 + row as f32 * 32.0;
            canvas.fill_rect(
                Rect::new(0.0, y, 1440.0, 32.0),
                Paint::fill(Rgba8::parse_hex("#1c1c1c").unwrap()),
            );
        }
    }
    println!(
        "27 row fills:        {:>8.1}µs",
        start.elapsed().as_nanos() as f64 / n as f64 / 1000.0
    );

    let mut runs = Vec::new();
    for row in 0..27 {
        for col in 0..6 {
            runs.push(engine.shape(&format!("cell-{row}-{col}"), &style, 240.0));
        }
    }
    let start = Instant::now();
    for _i in 0..n {
        for (index, shaped) in runs.iter().enumerate() {
            let row = index / 6;
            let col = index % 6;
            canvas.draw_text(
                &engine,
                shaped,
                14.0,
                Point::new(col as f32 * 240.0, 40.0 + row as f32 * 32.0),
                Rgba8::WHITE,
            );
        }
    }
    println!(
        "162 text runs:       {:>8.1}µs",
        start.elapsed().as_nanos() as f64 / n as f64 / 1000.0
    );

    let start = Instant::now();
    for _i in 0..n {
        for (index, shaped) in runs.iter().enumerate() {
            let _ = shaped;
            let _ = index;
        }
        let _ = engine.shape("cell-1-1", &style, 240.0);
    }
    println!(
        "162 shape lookups:   {:>8.1}µs",
        start.elapsed().as_nanos() as f64 / n as f64 / 1000.0
    );
}
