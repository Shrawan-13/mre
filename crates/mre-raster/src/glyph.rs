//! Glyph coverage bitmaps.
//!
//! A glyph is turned into a coverage bitmap once per size, device scale and
//! subpixel phase, then blitted. Drawing ten thousand rows of numbers is
//! then a memory copy per glyph instead of a path conversion per glyph.
//!
//! The coverage itself is the exact area of the outline inside each pixel.
//! The usual shortcut is to supersample the shape — four scanlines to a
//! pixel, as the scanline rasterisers do — and that is what a
//! supersampled glyph looks like: a stem that should be one pixel wide
//! comes out as a row of pixels with uneven weights, and the letters of a
//! word do not hold together. The exact area of the outline, which is what
//! a browser's rasteriser computes, gives every pixel the weight the shape
//! really gives it.

use crate::outline::{GlyphOutline, Segment};

/// One rasterised glyph, positioned relative to the pen.
pub struct GlyphBitmap {
    /// Bitmap width in physical pixels.
    pub width: u32,
    /// Bitmap height in physical pixels.
    pub height: u32,
    /// Device-pixel offset from the pen to the bitmap's left edge.
    pub left: f32,
    /// Device-pixel offset from the baseline to the bitmap's top edge.
    pub top: f32,
    /// Row-major coverage, `width * height` bytes.
    pub alpha: Vec<u8>,
}

/// Rasterises a glyph outline at a subpixel phase in `0.0..1.0`.
///
/// The phase is baked into the bitmap, so the blit lands on an integer pixel
/// while the outline keeps its subpixel position.
pub(crate) fn rasterize(outline: &GlyphOutline, phase: f32) -> Option<GlyphBitmap> {
    let width_f = outline.max_x - outline.min_x;
    let height_f = outline.max_y - outline.min_y;
    if width_f <= 0.0 || height_f <= 0.0 {
        return None;
    }
    let pad = 1.0;
    let width = width_f.ceil() as u32 + 2;
    let height = height_f.ceil() as u32 + 2;

    // The outline is moved so that its ink starts one pixel inside the
    // bitmap, and so that it keeps its fractional position on the screen.
    let dx = -outline.min_x + pad + phase;
    let dy = -outline.min_y + pad;
    let mut commands = Vec::with_capacity(outline.segments.len());
    for segment in &outline.segments {
        commands.push(match *segment {
            Segment::MoveTo(x, y) => zeno::Command::MoveTo([x + dx, y + dy].into()),
            Segment::LineTo(x, y) => zeno::Command::LineTo([x + dx, y + dy].into()),
            Segment::QuadTo(cx, cy, x, y) => {
                zeno::Command::QuadTo([cx + dx, cy + dy].into(), [x + dx, y + dy].into())
            }
            Segment::CurveTo(c1x, c1y, c2x, c2y, x, y) => zeno::Command::CurveTo(
                [c1x + dx, c1y + dy].into(),
                [c2x + dx, c2y + dy].into(),
                [x + dx, y + dy].into(),
            ),
            Segment::Close => zeno::Command::Close,
        });
    }

    let path = Path(commands);
    let mut alpha = vec![0u8; (width * height) as usize];
    let mut mask = zeno::Mask::new(&path);
    mask.style(zeno::Fill::NonZero);
    mask.origin(zeno::Origin::TopLeft);
    mask.size(width, height);
    mask.render_into(&mut alpha, None);

    Some(GlyphBitmap {
        width,
        height,
        left: outline.min_x - pad,
        top: outline.min_y - pad,
        alpha,
    })
}

/// A path the rasteriser can fill.
struct Path(Vec<zeno::Command>);

impl<'a> zeno::PathData for &'a Path {
    type Commands = std::iter::Copied<std::slice::Iter<'a, zeno::Command>>;

    fn commands(&self) -> Self::Commands {
        self.0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::Segment;

    fn square() -> GlyphOutline {
        let mut outline = GlyphOutline {
            segments: Vec::new(),
            min_x: 0.0,
            min_y: 0.0,
            max_x: 4.0,
            max_y: 4.0,
        };
        outline.segments.push(Segment::MoveTo(0.0, 0.0));
        outline.segments.push(Segment::LineTo(4.0, 0.0));
        outline.segments.push(Segment::LineTo(4.0, 4.0));
        outline.segments.push(Segment::LineTo(0.0, 4.0));
        outline.segments.push(Segment::Close);
        outline
    }

    #[test]
    fn a_square_has_coverage_and_padding() {
        let bitmap = rasterize(&square(), 0.0).expect("bitmap");
        assert_eq!(bitmap.width, 6);
        assert_eq!(bitmap.height, 6);
        assert_eq!(bitmap.left, -1.0);
        assert_eq!(bitmap.top, -1.0);
        let covered = bitmap.alpha.iter().filter(|alpha| **alpha > 0).count();
        assert_eq!(covered, 16);
    }

    #[test]
    fn a_square_is_fully_covered_in_the_middle() {
        let bitmap = rasterize(&square(), 0.0).expect("bitmap");
        // The four pixels of the square's own area are covered outright.
        for y in 1..5 {
            for x in 1..5 {
                assert_eq!(bitmap.alpha[(y * 6 + x) as usize], 255);
            }
        }
    }

    #[test]
    fn a_half_pixel_edge_is_half_covered() {
        // A square that is three and a half pixels wide: its right edge
        // falls in the middle of the fourth column, so that column holds
        // half a pixel's worth of ink.
        let mut outline = GlyphOutline {
            segments: Vec::new(),
            min_x: 0.0,
            min_y: 0.0,
            max_x: 3.5,
            max_y: 4.0,
        };
        outline.segments.push(Segment::MoveTo(0.0, 0.0));
        outline.segments.push(Segment::LineTo(3.5, 0.0));
        outline.segments.push(Segment::LineTo(3.5, 4.0));
        outline.segments.push(Segment::LineTo(0.0, 4.0));
        outline.segments.push(Segment::Close);
        let bitmap = rasterize(&outline, 0.0).expect("bitmap");
        let edge = bitmap.alpha[(2 * 6 + 4) as usize];
        assert!((120..=135).contains(&edge), "edge coverage {edge}");
    }

    #[test]
    fn a_zero_area_path_produces_nothing() {
        let mut outline = square();
        outline.max_x = 0.0;
        assert!(rasterize(&outline, 0.0).is_none());
    }
}
