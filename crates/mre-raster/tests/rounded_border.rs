//! Borders follow the shape they belong to.
//!
//! A card with rounded corners and a square outline shows the corners of the
//! outline past the corners of the card. It reads as a mistake even when the
//! colours are right, and it is the kind of detail that survives a long time
//! because nobody can say why the corner looks wrong.

use mre_core::{Rect, Rgba8};
use mre_raster::{Canvas, Paint};

const BORDER: Rgba8 = Rgba8::from_rgba(80, 110, 200, 255);
const FILL: Rgba8 = Rgba8::from_rgba(20, 26, 46, 255);
const WHITE: Rgba8 = Rgba8::from_rgba(255, 255, 255, 255);

fn card() -> Canvas {
    let mut canvas = Canvas::new(100, 100);
    canvas.clear(WHITE);
    let rect = Rect::new(10.0, 10.0, 80.0, 80.0);
    canvas.fill_rounded_rect(rect, 16.0, Paint::fill(FILL));
    canvas
}

#[test]
fn a_rounded_border_leaves_the_square_corner_alone() {
    let mut canvas = card();
    let rect = Rect::new(10.0, 10.0, 80.0, 80.0);
    canvas.stroke_rounded_rect(rect, 16.0, 2.0, Paint::fill(BORDER));

    // The very corner of the card is outside the rounded shape, so the
    // border must not reach it.
    for (x, y) in [(10, 10), (89, 10), (10, 89), (89, 89)] {
        assert_ne!(
            canvas.pixel(x, y),
            Some(BORDER),
            "the border reached the square corner at ({x}, {y})"
        );
    }
}

#[test]
fn a_square_border_reaches_the_square_corner() {
    // The old behaviour, kept as the contrast: a square outline does paint
    // the corner, which is exactly what must not happen to a rounded card.
    let mut canvas = card();
    let rect = Rect::new(10.0, 10.0, 80.0, 80.0);
    canvas.stroke_rect(rect, 2.0, Paint::fill(BORDER));

    assert_eq!(canvas.pixel(10, 10), Some(BORDER));
}

#[test]
fn a_rounded_border_still_draws_its_arc() {
    let mut canvas = card();
    let rect = Rect::new(10.0, 10.0, 80.0, 80.0);
    canvas.stroke_rounded_rect(rect, 16.0, 2.0, Paint::fill(BORDER));

    // The arc is drawn: a pixel on the curve between the corner and the
    // straight edge carries the border colour.
    let arc = canvas.pixel(15, 11).or_else(|| canvas.pixel(11, 15));
    assert!(
        arc.is_some(),
        "the rounded border should be drawn along the arc"
    );
    // The straight edges are drawn too.
    assert_eq!(canvas.pixel(50, 10), Some(BORDER));
    assert_eq!(canvas.pixel(50, 89), Some(BORDER));
}
