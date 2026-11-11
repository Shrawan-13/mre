//! The clip mask and the scissor must agree, pixel for pixel.
//!
//! Two paths clip a draw: a plain rectangle is cut by a scissor rectangle,
//! while a path (a rounded rectangle, a glyph) is cut by a coverage mask.
//! If the two disagree by even one pixel, a shape drawn over a background
//! leaves a hairline where the background shows through — the kind of seam
//! that is invisible in a test and obvious on screen.

use mre_core::{Rect, Rgba8};
use mre_raster::{Canvas, Paint};

const RED: Rgba8 = Rgba8::from_rgba(255, 0, 0, 255);
const WHITE: Rgba8 = Rgba8::from_rgba(255, 255, 255, 255);

/// The pixels a clip covers, in the same terms the canvas uses.
fn covered(clip: Rect) -> (i32, i32, i32, i32) {
    (
        clip.x.floor() as i32,
        clip.y.floor() as i32,
        (clip.x + clip.w).ceil() as i32,
        (clip.y + clip.h).ceil() as i32,
    )
}

#[test]
fn a_masked_shape_covers_the_same_pixels_as_a_clipped_one() {
    // A clip that starts on a fractional pixel: the case where a mask built
    // from the clip's *size* loses a row or a column.
    let clip = Rect::new(10.5, 10.5, 50.0, 20.25);
    let (x0, y0, x1, y1) = covered(clip);

    let mut masked = Canvas::new(100, 100);
    masked.clear(WHITE);
    masked.push_clip(clip);
    // A rounded rectangle takes the mask path.
    masked.fill_rounded_rect(Rect::new(0.0, 0.0, 100.0, 100.0), 8.0, Paint::fill(RED));
    masked.pop_clip();

    let mut clipped = Canvas::new(100, 100);
    clipped.clear(WHITE);
    clipped.push_clip(clip);
    // A plain rectangle takes the scissor path.
    clipped.fill_rect(Rect::new(0.0, 0.0, 100.0, 100.0), Paint::fill(RED));
    clipped.pop_clip();

    for y in y0..y1 {
        for x in x0..x1 {
            assert_eq!(
                masked.pixel(x as u32, y as u32),
                clipped.pixel(x as u32, y as u32),
                "pixel ({x}, {y}) differs between the masked and the clipped fill"
            );
        }
    }
}

#[test]
fn a_masked_shape_fills_the_whole_clip() {
    let clip = Rect::new(10.5, 10.5, 50.0, 20.25);
    let (x0, y0, x1, y1) = covered(clip);

    let mut canvas = Canvas::new(100, 100);
    canvas.clear(WHITE);
    canvas.push_clip(clip);
    canvas.fill_rounded_rect(Rect::new(0.0, 0.0, 100.0, 100.0), 8.0, Paint::fill(RED));
    canvas.pop_clip();

    // Every pixel of the clip is inside the shape, so all of them are red.
    for y in y0..y1 {
        for x in x0..x1 {
            assert_eq!(
                canvas.pixel(x as u32, y as u32),
                Some(RED),
                "pixel ({x}, {y}) was not filled"
            );
        }
    }
}
