//! Glyph coverage bitmaps.
//!
//! A glyph path is converted to an alpha bitmap once per size, device scale and
//! quarter-pixel phase, then blitted. Drawing ten thousand rows of numbers is
//! then a memory copy per glyph instead of a path conversion per glyph.

use tiny_skia::{FillRule, Paint, Path, Pixmap, Transform};

/// One rasterised glyph, positioned relative to the pen.
pub(crate) struct GlyphBitmap {
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

/// Rasterises a glyph path at a subpixel phase in `0.0..1.0`.
///
/// The phase is baked into the bitmap, so the blit lands on an integer pixel
/// while the outline keeps its subpixel position.
pub(crate) fn rasterize(path: &Path, phase: f32, phase_y: f32) -> Option<GlyphBitmap> {
    let bounds = path.bounds();
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return None;
    }
    let pad = 1.0;
    let width = bounds.width().ceil() as u32 + 2;
    let height = bounds.height().ceil() as u32 + 2;
    let mut pixmap = Pixmap::new(width, height)?;

    let mut paint = Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    paint.anti_alias = true;
    pixmap.fill_path(
        path,
        &paint,
        FillRule::Winding,
        Transform::from_translate(-bounds.x() + pad + phase, -bounds.y() + pad + phase_y),
        None,
    );

    let mut alpha = Vec::with_capacity((width * height) as usize);
    alpha.extend(pixmap.data().chunks_exact(4).map(|pixel| pixel[3]));

    Some(GlyphBitmap {
        width,
        height,
        left: bounds.x() - pad,
        top: bounds.y() - pad,
        alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::PathBuilder;

    #[test]
    fn a_square_has_coverage_and_padding() {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        builder.line_to(4.0, 0.0);
        builder.line_to(4.0, 4.0);
        builder.line_to(0.0, 4.0);
        builder.close();
        let path = builder.finish().expect("path");
        let bitmap = rasterize(&path, 0.0, 0.0).expect("bitmap");
        assert_eq!(bitmap.width, 6);
        assert_eq!(bitmap.height, 6);
        assert_eq!(bitmap.left, -1.0);
        assert_eq!(bitmap.top, -1.0);
        let covered = bitmap.alpha.iter().filter(|alpha| **alpha > 0).count();
        assert_eq!(covered, 16);
    }

    #[test]
    fn a_zero_area_path_produces_nothing() {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        builder.line_to(4.0, 0.0);
        let path = builder.finish().expect("path");
        assert!(rasterize(&path, 0.0, 0.0).is_none());
    }
}
