//! Clipping behaviour, which scroll containers depend on.
//!
//! Clipping is exact, including for text and rounded shapes, because it uses a
//! coverage mask rather than a scissor rectangle.

use mre_core::{Point, Rect, Rgba8};
use mre_raster::{Canvas, Paint};
use mre_text::{FontDesc, TextEngine, TextStyle};

const RED: Rgba8 = Rgba8::from_rgba(255, 0, 0, 255);

#[test]
fn a_clip_cuts_a_rectangle() {
    let mut canvas = Canvas::new(200, 200);
    canvas.clear(Rgba8::WHITE);

    canvas.push_clip(Rect::new(50.0, 50.0, 100.0, 100.0));
    canvas.fill_rect(Rect::new(0.0, 0.0, 200.0, 200.0), Paint::fill(RED));
    canvas.pop_clip();

    assert_eq!(canvas.pixel(25, 25), Some(Rgba8::WHITE));
    assert_eq!(canvas.pixel(75, 75), Some(RED));
    assert_eq!(canvas.pixel(175, 175), Some(Rgba8::WHITE));
}

#[test]
fn nested_clips_intersect() {
    let mut canvas = Canvas::new(200, 200);
    canvas.clear(Rgba8::WHITE);

    canvas.push_clip(Rect::new(0.0, 0.0, 120.0, 200.0));
    canvas.push_clip(Rect::new(0.0, 80.0, 200.0, 60.0));
    canvas.fill_rect(Rect::new(0.0, 0.0, 200.0, 200.0), Paint::fill(RED));
    canvas.pop_clip();
    canvas.pop_clip();

    assert_eq!(canvas.clip_depth(), 0);
    assert_eq!(canvas.pixel(60, 100), Some(RED));
    assert_eq!(
        canvas.pixel(160, 100),
        Some(Rgba8::WHITE),
        "x clipped at 120"
    );
    assert_eq!(canvas.pixel(60, 40), Some(Rgba8::WHITE), "y clipped at 80");
    assert_eq!(
        canvas.pixel(60, 160),
        Some(Rgba8::WHITE),
        "y clipped at 140"
    );
}

#[test]
fn a_clip_survives_a_scale_change() {
    let mut canvas = Canvas::new(400, 400);
    canvas.set_scale(2.0);
    canvas.clear(Rgba8::WHITE);

    // Logical (50, 50, 100, 100) is physical (100, 100, 200, 200).
    canvas.push_clip(Rect::new(50.0, 50.0, 100.0, 100.0));
    canvas.fill_rect(Rect::new(0.0, 0.0, 400.0, 400.0), Paint::fill(RED));
    canvas.pop_clip();

    assert_eq!(canvas.pixel(150, 150), Some(RED));
    assert_eq!(canvas.pixel(50, 50), Some(Rgba8::WHITE));
    assert_eq!(canvas.pixel(350, 350), Some(Rgba8::WHITE));
}

#[test]
fn clipping_is_restored_after_a_pop() {
    let mut canvas = Canvas::new(100, 100);
    canvas.clear(Rgba8::WHITE);

    canvas.push_clip(Rect::new(0.0, 0.0, 10.0, 10.0));
    canvas.pop_clip();

    canvas.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Paint::fill(RED));
    assert_eq!(canvas.pixel(90, 90), Some(RED));
}

#[test]
fn text_is_clipped_at_the_boundary() {
    let mut engine = TextEngine::new();
    engine
        .add_font_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/Roboto-Regular.ttf"
            ),
            FontDesc::new("Roboto"),
        )
        .expect("font");
    let shaped = engine.shape("MMMMMMMMMMMMMMMM", &TextStyle::new(24.0), 400.0);

    let mut canvas = Canvas::new(200, 100);
    canvas.clear(Rgba8::WHITE);
    canvas.push_clip(Rect::new(0.0, 0.0, 40.0, 100.0));
    canvas.draw_text(&engine, &shaped, 24.0, Point::new(0.0, 10.0), Rgba8::BLACK);
    canvas.pop_clip();

    let mut clipped = 0;
    let mut visible = 0;
    for y in 0..40 {
        for x in 0..200 {
            if canvas.pixel(x, y) == Some(Rgba8::BLACK) {
                if x < 40 {
                    visible += 1;
                } else {
                    clipped += 1;
                }
            }
        }
    }

    assert!(visible > 0, "text inside the clip should be drawn");
    assert_eq!(clipped, 0, "text outside the clip must not leak");
}
