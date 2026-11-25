//! The reference rasteriser as a [`Backend`].
//!
//! The methods are thin delegations to `Canvas`'s inherent methods. Keeping
//! the real rasterisation in `lib.rs` means the golden tests exercise exactly
//! the code the software backend calls — there is no second implementation
//! that could drift.

use crate::Canvas;
use mre_core::{Point, Rect, Rgba8};
use mre_paint::{Backend, BackendKind, Paint};
use mre_text::{ShapedText, TextEngine};

impl Backend for Canvas {
    fn kind(&self) -> BackendKind {
        BackendKind::Software
    }

    fn width(&self) -> u32 {
        Canvas::width(self)
    }

    fn height(&self) -> u32 {
        Canvas::height(self)
    }

    fn scale(&self) -> f32 {
        Canvas::scale(self)
    }

    fn set_scale(&mut self, scale: f32) {
        Canvas::set_scale(self, scale);
    }

    fn resize(&mut self, width: u32, height: u32) {
        let scale = Canvas::scale(self);
        *self = Canvas::new(width, height);
        Canvas::set_scale(self, scale);
    }

    fn calls(&self) -> u64 {
        self.calls
    }

    fn clear(&mut self, color: Rgba8) {
        Canvas::clear(self, color);
    }

    fn clear_rect(&mut self, rect: Rect, color: Rgba8) {
        Canvas::clear_rect(self, rect, color);
    }

    fn push_clip(&mut self, rect: Rect) {
        Canvas::push_clip(self, rect);
    }

    fn pop_clip(&mut self) {
        Canvas::pop_clip(self);
    }

    fn fill_rect(&mut self, rect: Rect, paint: Paint) {
        Canvas::fill_rect(self, rect, paint);
    }

    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, paint: Paint) {
        Canvas::fill_rounded_rect(self, rect, radius, paint);
    }

    fn stroke_rect(&mut self, rect: Rect, width: f32, paint: Paint) {
        Canvas::stroke_rect(self, rect, width, paint);
    }

    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, width: f32, paint: Paint) {
        Canvas::stroke_rounded_rect(self, rect, radius, width, paint);
    }

    fn fill_circle(&mut self, center: Point, radius: f32, paint: Paint) {
        Canvas::fill_circle(self, center, radius, paint);
    }

    fn draw_line(&mut self, from: Point, to: Point, width: f32, paint: Paint) {
        Canvas::draw_line(self, from, to, width, paint);
    }

    fn draw_text(
        &mut self,
        engine: &TextEngine,
        shaped: &ShapedText,
        size: f32,
        origin: Point,
        color: Rgba8,
    ) {
        Canvas::draw_text(self, engine, shaped, size, origin, color);
    }

    fn copy_region(&mut self, rect: Rect, dx: i32, dy: i32) -> bool {
        Canvas::copy_region(self, rect, dx, dy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_is_the_software_backend() {
        let mut canvas = Canvas::new(8, 8);
        {
            let backend: &mut dyn Backend = &mut canvas;
            assert_eq!(backend.kind(), BackendKind::Software);
            assert_eq!((backend.width(), backend.height()), (8, 8));
            backend.set_scale(2.0);
            assert_eq!(backend.scale(), 2.0);
            backend.fill_rect(Rect::new(1.0, 1.0, 2.0, 2.0), Paint::fill(Rgba8::WHITE));
            backend.finish();
            assert_eq!(backend.calls(), 1);
        }
        assert_eq!(canvas.pixel(3, 3), Some(Rgba8::WHITE));
        {
            let backend: &mut dyn Backend = &mut canvas;
            backend.resize(4, 4);
            assert_eq!((backend.width(), backend.height()), (4, 4));
            assert_eq!(
                backend.scale(),
                2.0,
                "resize must preserve the device scale"
            );
        }
    }
}
