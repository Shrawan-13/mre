//! Rasteriser behaviour and the golden image.

use mre_core::{Point, Rect, Rgba8};
use mre_raster::{Canvas, Paint, demo_card};

const GOLDEN: &[u8] = include_bytes!("../../../assets/golden/text-card.rgba");

fn font_path() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    ))
    .to_path_buf()
}

#[test]
fn clear_fills_every_pixel() {
    let mut canvas = Canvas::new(8, 4);
    canvas.clear(Rgba8::from_rgba(10, 20, 30, 255));

    for y in 0..4 {
        for x in 0..8 {
            assert_eq!(canvas.pixel(x, y), Some(Rgba8::from_rgba(10, 20, 30, 255)));
        }
    }
}

#[test]
fn fill_rect_only_touches_inside_pixels() {
    let mut canvas = Canvas::new(10, 10);
    canvas.clear(Rgba8::WHITE);
    canvas.fill_rect(Rect::new(2.0, 2.0, 4.0, 4.0), Paint::fill(Rgba8::BLACK));

    assert_eq!(canvas.pixel(0, 0), Some(Rgba8::WHITE));
    assert_eq!(canvas.pixel(3, 3), Some(Rgba8::BLACK));
    // The right/bottom edges are exclusive, matching the layout rect contract.
    assert_eq!(canvas.pixel(6, 6), Some(Rgba8::WHITE));
}

#[test]
fn rounded_corners_are_not_filled() {
    let mut canvas = Canvas::new(20, 20);
    canvas.clear(Rgba8::WHITE);
    canvas.fill_rounded_rect(
        Rect::new(0.0, 0.0, 20.0, 20.0),
        8.0,
        Paint::fill(Rgba8::BLACK),
    );

    // The exact corner is outside the radius and must stay background.
    assert_eq!(canvas.pixel(0, 0), Some(Rgba8::WHITE));
    // The centre must be filled.
    assert_eq!(canvas.pixel(10, 10), Some(Rgba8::BLACK));
}

#[test]
fn circle_is_round() {
    let mut canvas = Canvas::new(40, 40);
    canvas.clear(Rgba8::WHITE);
    canvas.fill_circle(Point::new(20.0, 20.0), 10.0, Paint::fill(Rgba8::BLACK));

    assert_eq!(canvas.pixel(20, 20), Some(Rgba8::BLACK));
    assert_eq!(canvas.pixel(20, 11), Some(Rgba8::BLACK));
    // A corner of the bounding box is outside the circle.
    assert_eq!(canvas.pixel(11, 11), Some(Rgba8::WHITE));
}

#[test]
fn pixel_reads_outside_the_canvas_return_none() {
    let canvas = Canvas::new(4, 4);
    assert_eq!(canvas.pixel(4, 0), None);
    assert_eq!(canvas.pixel(0, 4), None);
}

#[test]
fn text_marks_pixels() {
    let canvas = demo_card(&font_path());
    let dark = canvas
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[0] < 80 && pixel[1] < 80 && pixel[2] < 80)
        .count();

    assert!(
        dark > 100,
        "text should darken a meaningful number of pixels, got {dark}"
    );
}

#[test]
fn golden_image_matches_byte_for_byte() {
    let canvas = demo_card(&font_path());

    assert_eq!(
        canvas.pixels().len(),
        GOLDEN.len(),
        "golden image has the wrong dimensions"
    );

    let mismatches = canvas
        .pixels()
        .iter()
        .zip(GOLDEN)
        .filter(|(actual, expected)| actual != expected)
        .count();

    if mismatches > 0 {
        // Dump the render so a human can look at what changed.
        let _ = canvas.save_png("/tmp/mre-golden-actual.png");
    }

    assert_eq!(
        mismatches, 0,
        "rendering drifted from the reference; see /tmp/mre-golden-actual.png"
    );
}

#[test]
fn zero_sized_canvases_are_clamped() {
    let canvas = Canvas::new(0, 0);
    assert_eq!(canvas.width(), 1);
    assert_eq!(canvas.height(), 1);
}
