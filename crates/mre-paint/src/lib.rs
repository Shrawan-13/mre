//! The renderer contract.
//!
//! Everything above this crate — layout, text, widgets, the reactive graph and
//! Lua — is written once against [`Backend`]. The software rasteriser and the
//! GPU renderer are two implementations of the same surface-shaped API, so a
//! switch of hardware does not mean a rewrite of the engine.
//!
//! The shape of the API is deliberately close to a canvas: damage tracking,
//! the clip stack and the scroll blit live *above* it, in `mre-widgets`. That
//! is the expensive logic to get right, and both backends share it. The
//! backends only turn already-decided operations into pixels.
//!
//! One frame is: `push_clip`/draw/`pop_clip` … then exactly one [`Backend::finish`].

use mre_core::{Point, Rect, Rgba8};
use mre_text::{ShapedText, TextEngine};

/// An opaque-ish fill description. Kept as its own type so call sites read the
/// same whether the pixels end up in a `Pixmap` or a vertex buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Paint {
    /// Solid colour, sRGB with alpha.
    pub color: Rgba8,
}

impl Paint {
    /// A solid fill.
    #[must_use]
    pub const fn fill(color: Rgba8) -> Self {
        Self { color }
    }
}

/// Which implementation is drawing. Hosts use this for diagnostics and for
/// picking a fallback when no usable GPU exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// Deterministic CPU reference rasteriser.
    Software,
    /// Hardware-accelerated renderer.
    Gpu,
}

/// A surface-shaped rendering backend.
///
/// Coordinates are logical pixels; `scale` converts them to physical pixels.
/// Clips are pushed and popped by the paint pass and intersect, exactly as in
/// the reference rasteriser.
pub trait Backend {
    /// The implementation behind this surface.
    fn kind(&self) -> BackendKind;

    /// Surface width in physical pixels.
    fn width(&self) -> u32;

    /// Surface height in physical pixels.
    fn height(&self) -> u32;

    /// Logical-to-physical scale.
    fn scale(&self) -> f32;

    /// Changes the logical-to-physical scale.
    fn set_scale(&mut self, scale: f32);

    /// Reallocates the surface. The contents are undefined afterwards, so the
    /// caller must damage everything.
    fn resize(&mut self, width: u32, height: u32);

    /// Draw calls recorded this frame, for diagnostics.
    fn calls(&self) -> u64;

    /// Fills the whole surface.
    fn clear(&mut self, color: Rgba8);

    /// Fills one rectangle.
    fn clear_rect(&mut self, rect: Rect, color: Rgba8);

    /// Pushes a clip rectangle, in logical pixels. Clips intersect.
    fn push_clip(&mut self, rect: Rect);

    /// Pops the most recent clip rectangle.
    fn pop_clip(&mut self);

    /// Fills an axis-aligned rectangle.
    fn fill_rect(&mut self, rect: Rect, paint: Paint);

    /// Fills a rectangle with rounded corners.
    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, paint: Paint);

    /// Strokes a rectangle outline.
    fn stroke_rect(&mut self, rect: Rect, width: f32, paint: Paint);

    /// Strokes a rounded rectangle outline.
    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, width: f32, paint: Paint);

    /// Fills a circle.
    fn fill_circle(&mut self, center: Point, radius: f32, paint: Paint);

    /// Strokes a circle outline. A square node whose radius is half its side
    /// is a circle, and must not go through the rounded-rect path: four
    /// quadratic corners are visibly not circular at knob sizes.
    fn stroke_circle(&mut self, center: Point, radius: f32, width: f32, paint: Paint);

    /// Draws a line with a given thickness.
    fn draw_line(&mut self, from: Point, to: Point, width: f32, paint: Paint);

    /// Draws shaped text at `origin` (the top of the block).
    fn draw_text(
        &mut self,
        engine: &TextEngine,
        shaped: &ShapedText,
        size: f32,
        origin: Point,
        color: Rgba8,
    );

    /// Copies a region by an offset, in physical pixels. Returns `false` when
    /// the shift cannot be done (out of bounds) and the caller must repaint.
    fn copy_region(&mut self, rect: Rect, dx: i32, dy: i32) -> bool;

    /// Ends the frame. The software backend does nothing; the GPU backend
    /// performs its one and only submit and presents the surface.
    fn finish(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_is_small_and_copy() {
        assert!(core::mem::size_of::<Paint>() <= 8);
        let a = Paint::fill(Rgba8::WHITE);
        let b = a;
        assert_eq!(a, b);
    }
}
