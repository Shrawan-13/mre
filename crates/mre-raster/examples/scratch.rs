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
        size: 13.0,
        ..TextStyle::default()
    };
    let shaped = engine.shape("Instrument 0042", &style, 240.0);

    let mut canvas = Canvas::new(1000, 760);
    let whole = Rect::new(0.0, 0.0, 1000.0, 760.0);
    let row = Rect::new(0.0, 0.0, 1000.0, 28.0);

    let n = 500;

    let start = Instant::now();
    for _ in 0..n {
        canvas.clear(Rgba8::WHITE);
    }
    println!("clear full:      {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for i in 0..n {
        canvas.fill_rect(
            Rect::new(0.0, (i % 20) as f32 * 28.0, 1000.0, 28.0),
            Paint::fill(Rgba8::BLACK),
        );
    }
    println!("fill 1000x28:    {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for i in 0..n {
        canvas.fill_rounded_rect(
            Rect::new(0.0, (i % 20) as f32 * 28.0, 1000.0, 28.0),
            0.0,
            Paint::fill(Rgba8::BLACK),
        );
    }
    println!("rounded 1000x28: {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for i in 0..n {
        canvas.push_clip(Rect::new(0.0, 0.0, 900.0, 600.0));
        canvas.fill_rect(
            Rect::new(0.0, (i % 20) as f32 * 28.0, 1000.0, 28.0),
            Paint::fill(Rgba8::BLACK),
        );
        canvas.pop_clip();
    }
    println!("clipped fill:    {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for i in 0..n {
        canvas.draw_text(
            &engine,
            &shaped,
            13.0,
            Point::new(10.0, (i % 20) as f32 * 28.0),
            Rgba8::BLACK,
        );
    }
    println!("text run (17ch): {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for i in 0..n {
        canvas.push_clip(Rect::new(0.0, 0.0, 900.0, 600.0));
        canvas.draw_text(
            &engine,
            &shaped,
            13.0,
            Point::new(10.0, (i % 20) as f32 * 28.0),
            Rgba8::BLACK,
        );
        canvas.pop_clip();
    }
    println!("clipped text:    {:?}/ea", start.elapsed() / n);

    let one = engine.shape("I", &style, 240.0);
    let start = Instant::now();
    for _ in 0..n {
        canvas.draw_text(&engine, &one, 13.0, Point::new(10.0, 100.0), Rgba8::BLACK);
    }
    println!("text run (1ch):  {:?}/ea", start.elapsed() / n);

    let long: String = "Instrument 0042 abcdefghijklmnop".to_string();
    let long = engine.shape(&long, &style, 400.0);
    let start = Instant::now();
    for _ in 0..n {
        canvas.draw_text(&engine, &long, 13.0, Point::new(10.0, 100.0), Rgba8::BLACK);
    }
    println!("text run (35ch): {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for _ in 0..n {
        canvas.fill_rounded_rect(
            Rect::new(10.0, 100.0, 200.0, 13.0),
            3.0,
            Paint::fill(Rgba8::BLACK),
        );
    }
    println!("rounded 200x13:  {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for _ in 0..n {
        canvas.fill_rect(
            Rect::new(10.0, 100.0, 10.0, 10.0),
            Paint::fill(Rgba8::BLACK),
        );
    }
    println!("fill 10x10:      {:?}/ea", start.elapsed() / n);

    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(10.0, 100.0);
    builder.line_to(40.0, 100.0);
    builder.line_to(25.0, 120.0);
    builder.close();
    let triangle = builder.finish().unwrap();
    let start = Instant::now();
    for _ in 0..n {
        canvas.fill_path(&triangle, Paint::fill(Rgba8::BLACK));
    }
    println!("triangle:        {:?}/ea", start.elapsed() / n);

    let big = TextStyle {
        size: 26.0,
        ..TextStyle::default()
    };
    let one_big = engine.shape("I", &big, 240.0);
    let start = Instant::now();
    for _ in 0..n {
        canvas.draw_text(
            &engine,
            &one_big,
            26.0,
            Point::new(10.0, 100.0),
            Rgba8::BLACK,
        );
    }
    println!("text 26px 1ch:   {:?}/ea", start.elapsed() / n);

    let start = Instant::now();
    for _ in 0..n {
        canvas.draw_text(&engine, &one, 13.0, Point::new(10.0, 100.0), Rgba8::BLACK);
    }
    println!("text 13px 1ch:   {:?}/ea", start.elapsed() / n);

    let _ = whole;
    let _ = row;
}
