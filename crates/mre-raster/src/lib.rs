//! The reference rasteriser: deterministic pixels with no GPU in the loop.
//!
//! This exists for three reasons, in order of importance:
//!
//! 1. **Golden tests.** If the same scene produces the same pixels on every
//!    platform, a rendering regression is a failing test rather than a bug
//!    report.
//! 2. **A correctness oracle for the GPU backend.** The GPU path must match this
//!    output within a tolerance; without a reference there is nothing to match.
//! 3. **A backend of last resort.** Machines without a usable GPU still render.
//!
//! It is not the performance path. Glyph outlines are cached as paths, rects are
//! analytic, and the whole thing runs on one thread.

use mre_core::{Point, Rect, Rgba8};
use mre_text::{ShapedText, TextEngine};
use std::collections::HashMap;
use tiny_skia::{FillRule, Paint as SkiaPaint, Path, PathBuilder, Pixmap, Transform};

/// A fill description.
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

/// A CPU-rendered surface.
pub struct Canvas {
    pixmap: Pixmap,
    glyphs: HashMap<GlyphKey, Option<Path>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u32,
    glyph: u16,
    size: u32,
}

impl std::fmt::Debug for Canvas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Canvas")
            .field("width", &self.pixmap.width())
            .field("height", &self.pixmap.height())
            .field("cached_glyphs", &self.glyphs.len())
            .finish()
    }
}

impl Canvas {
    /// Creates a transparent canvas. Zero dimensions are clamped to one pixel so
    /// a minimised window cannot produce an invalid surface.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let mut pixmap = Pixmap::new(width, height).expect("canvas dimensions are non-zero");
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        Self {
            pixmap,
            glyphs: HashMap::new(),
        }
    }

    /// Width in physical pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    /// Height in physical pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    /// Number of cached glyph outlines.
    #[must_use]
    pub fn cached_glyphs(&self) -> usize {
        self.glyphs.len()
    }

    /// Fills the whole surface.
    pub fn clear(&mut self, color: Rgba8) {
        self.pixmap.fill(to_skia(color));
    }

    /// Raw premultiplied RGBA pixels, row-major.
    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Encodes the canvas as a PNG.
    #[must_use]
    pub fn encode_png(&self) -> Vec<u8> {
        self.pixmap.encode_png().unwrap_or_default()
    }

    /// Writes the canvas to a PNG file.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the file cannot be written.
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.encode_png())
    }

    /// Fills a rectangle.
    pub fn fill_rect(&mut self, rect: Rect, paint: Paint) {
        let Some(path) = rect_path(rect, 0.0) else {
            return;
        };
        self.fill_path(&path, paint);
    }

    /// Fills a rounded rectangle.
    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, paint: Paint) {
        let Some(path) = rect_path(rect, radius) else {
            return;
        };
        self.fill_path(&path, paint);
    }

    /// Strokes a rectangle outline inside its bounds.
    pub fn stroke_rect(&mut self, rect: Rect, width: f32, paint: Paint) {
        if width <= 0.0 {
            return;
        }
        let Some(path) = rect_path(rect, 0.0) else {
            return;
        };
        let mut skia = to_skia_paint(paint);
        skia.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width,
            ..tiny_skia::Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &skia, &stroke, Transform::identity(), None);
    }

    /// Fills a circle.
    pub fn fill_circle(&mut self, center: Point, radius: f32, paint: Paint) {
        if radius <= 0.0 {
            return;
        }
        let Some(path) = ellipse_path(center, radius, radius) else {
            return;
        };
        self.fill_path(&path, paint);
    }

    /// Draws a straight line with a given thickness.
    pub fn draw_line(&mut self, from: Point, to: Point, width: f32, paint: Paint) {
        if width <= 0.0 {
            return;
        }
        let mut builder = PathBuilder::new();
        builder.move_to(from.x, from.y);
        builder.line_to(to.x, to.y);
        let Some(path) = builder.finish() else {
            return;
        };
        let mut skia = to_skia_paint(paint);
        skia.anti_alias = true;
        let stroke = tiny_skia::Stroke {
            width,
            ..tiny_skia::Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &skia, &stroke, Transform::identity(), None);
    }

    /// Fills an arbitrary path built by the caller.
    pub fn fill_path(&mut self, path: &Path, paint: Paint) {
        let skia = to_skia_paint(paint);
        self.pixmap
            .fill_path(path, &skia, FillRule::Winding, Transform::identity(), None);
    }

    /// Draws shaped text at `origin`, where `origin.y` is the top of the block.
    ///
    /// Glyph outlines are cached by `(font, glyph, size)`, so repeated text is a
    /// path lookup plus a fill rather than a font parse.
    pub fn draw_text(
        &mut self,
        engine: &TextEngine,
        shaped: &ShapedText,
        size: f32,
        origin: Point,
        color: Rgba8,
    ) {
        let skia = to_skia_paint(Paint::fill(color));
        for glyph in &shaped.glyphs {
            let key = GlyphKey {
                font: glyph.font.0,
                glyph: glyph.glyph_id,
                size: (size * 64.0).round().max(0.0) as u32,
            };
            let path = self
                .glyphs
                .entry(key)
                .or_insert_with(|| glyph_path(engine, glyph.font, glyph.glyph_id, size));
            let Some(path) = path else {
                continue;
            };
            let transform = Transform::from_translate(
                origin.x + glyph.x + glyph.x_offset,
                origin.y + glyph.y - glyph.y_offset,
            );
            self.pixmap
                .fill_path(path, &skia, FillRule::Winding, transform, None);
        }
    }

    /// Reads one pixel as straight (non-premultiplied) RGBA.
    #[must_use]
    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgba8> {
        if x >= self.width() || y >= self.height() {
            return None;
        }
        let index = ((y * self.width() + x) * 4) as usize;
        let data = self.pixmap.data();
        let alpha = data[index + 3];
        if alpha == 0 {
            return Some(Rgba8::TRANSPARENT);
        }
        // Pixmap data is premultiplied; undo it for a predictable test surface.
        let unpremultiply = |channel: u8| -> u8 {
            let value = (u32::from(channel) * 255) / u32::from(alpha);
            u8::try_from(value.min(255)).unwrap_or(255)
        };
        Some(Rgba8::from_rgba(
            unpremultiply(data[index]),
            unpremultiply(data[index + 1]),
            unpremultiply(data[index + 2]),
            alpha,
        ))
    }
}

fn to_skia(color: Rgba8) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color.r(), color.g(), color.b(), color.a())
}

fn to_skia_paint(paint: Paint) -> SkiaPaint<'static> {
    let mut skia = SkiaPaint::default();
    skia.set_color_rgba8(
        paint.color.r(),
        paint.color.g(),
        paint.color.b(),
        paint.color.a(),
    );
    skia.anti_alias = true;
    skia
}

fn rect_path(rect: Rect, radius: f32) -> Option<Path> {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let radius = radius.max(0.0).min(rect.w / 2.0).min(rect.h / 2.0);
    let mut builder = PathBuilder::new();

    if radius <= 0.0 {
        builder.move_to(rect.x, rect.y);
        builder.line_to(rect.right(), rect.y);
        builder.line_to(rect.right(), rect.bottom());
        builder.line_to(rect.x, rect.bottom());
        builder.close();
        return builder.finish();
    }

    let (l, t, r, b) = (rect.x, rect.y, rect.right(), rect.bottom());
    builder.move_to(l + radius, t);
    builder.line_to(r - radius, t);
    builder.quad_to(r, t, r, t + radius);
    builder.line_to(r, b - radius);
    builder.quad_to(r, b, r - radius, b);
    builder.line_to(l + radius, b);
    builder.quad_to(l, b, l, b - radius);
    builder.line_to(l, t + radius);
    builder.quad_to(l, t, l + radius, t);
    builder.close();
    builder.finish()
}

fn ellipse_path(center: Point, rx: f32, ry: f32) -> Option<Path> {
    let mut builder = PathBuilder::new();
    // Four cubic segments approximate an ellipse to well under a pixel.
    const KAPPA: f32 = 0.552_284_8;
    let (cx, cy) = (center.x, center.y);
    let kx = rx * KAPPA;
    let ky = ry * KAPPA;

    builder.move_to(cx, cy - ry);
    builder.cubic_to(cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy);
    builder.cubic_to(cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry);
    builder.cubic_to(cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy);
    builder.cubic_to(cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry);
    builder.close();
    builder.finish()
}

fn glyph_path(engine: &TextEngine, font: mre_text::FontId, glyph: u16, size: f32) -> Option<Path> {
    let upem = engine.units_per_em(font).max(1.0);
    let scale = size / upem;

    engine.with_font_data(font, |data, index| {
        let face = ttf_parser::Face::parse(data, index).ok()?;
        let mut sink = OutlineSink {
            builder: PathBuilder::new(),
            scale,
        };
        face.outline_glyph(ttf_parser::GlyphId(glyph), &mut sink)?;
        sink.builder.finish()
    })?
}

struct OutlineSink {
    builder: PathBuilder,
    scale: f32,
}

impl OutlineSink {
    fn sx(&self, x: f32) -> f32 {
        x * self.scale
    }

    fn sy(&self, y: f32) -> f32 {
        -y * self.scale
    }
}

impl ttf_parser::OutlineBuilder for OutlineSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to(self.sx(x), self.sy(y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(self.sx(x), self.sy(y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder
            .quad_to(self.sx(x1), self.sy(y1), self.sx(x), self.sy(y));
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            self.sx(x1),
            self.sy(y1),
            self.sx(x2),
            self.sy(y2),
            self.sx(x),
            self.sy(y),
        );
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

/// Renders the reference card used by the golden test and the example.
///
/// Keeping the scene in one place means the example cannot drift from the test
/// and leave a stale golden image behind.
#[must_use]
pub fn demo_card(font_path: &std::path::Path) -> Canvas {
    let mut canvas = Canvas::new(240, 120);
    canvas.clear(Rgba8::WHITE);
    canvas.fill_rounded_rect(
        Rect::new(8.0, 8.0, 224.0, 104.0),
        12.0,
        Paint::fill(Rgba8::parse_hex("#F2F4F8").expect("valid colour")),
    );
    canvas.stroke_rect(
        Rect::new(8.5, 8.5, 223.0, 103.0),
        1.0,
        Paint::fill(Rgba8::parse_hex("#D7DCE5").expect("valid colour")),
    );

    let mut engine = TextEngine::new();
    if engine
        .add_font_file(font_path, mre_text::FontDesc::new("Roboto"))
        .is_ok()
    {
        let title = engine.shape("MRE", &mre_text::TextStyle::new(32.0), f32::INFINITY);
        canvas.draw_text(
            &engine,
            &title,
            32.0,
            Point::new(24.0, 26.0),
            Rgba8::parse_hex("#111827").expect("valid colour"),
        );

        let caption = engine.shape(
            "rendered on the CPU",
            &mre_text::TextStyle::new(13.0),
            f32::INFINITY,
        );
        canvas.draw_text(
            &engine,
            &caption,
            13.0,
            Point::new(24.0, 68.0),
            Rgba8::parse_hex("#6B7280").expect("valid colour"),
        );
    }

    canvas.fill_circle(
        Point::new(196.0, 60.0),
        22.0,
        Paint::fill(Rgba8::parse_hex("#2563EB").expect("valid colour")),
    );

    canvas
}
