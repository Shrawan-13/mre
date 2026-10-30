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

mod glyph;
mod pixel;

use glyph::GlyphBitmap;
use mre_core::{Point, Rect, Rgba8};
use mre_text::{ShapedText, TextEngine};
use std::collections::HashMap;
use tiny_skia::{FillRule, Mask, Paint as SkiaPaint, Path, PathBuilder, Pixmap, Transform};

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
    glyphs: HashMap<GlyphKey, Option<GlyphBitmap>>,
    scale: f32,
    /// Active clip rectangles, in logical pixels. Their intersection is the
    /// visible region.
    clip: Vec<Rect>,
    /// Reusable coverage mask. Kept across frames so clipping does not allocate.
    mask: Option<Mask>,
    /// The physical rect the mask currently represents, when clipping is active.
    mask_rect: Option<Rect>,
    /// How many times a coverage mask has been built. Shapes that fit inside the
    /// clip bypass the mask entirely, so this stays flat for the common case.
    mask_builds: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    font: u32,
    glyph: u16,
    size: u32,
    /// Device-scale bucket, so a DPI change rasterises once per scale.
    scale: u32,
    /// Subpixel x phase, in quarters.
    phase: u8,
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
            scale: 1.0,
            clip: Vec::new(),
            mask: None,
            mask_rect: None,
            mask_builds: 0,
        }
    }

    /// Pushes a clip rectangle, in logical pixels.
    ///
    /// Clips intersect, so nested scroll containers compose. Clipping is exact:
    /// it uses a coverage mask, not a scissor, so text and rounded shapes are
    /// cut correctly at the boundary.
    pub fn push_clip(&mut self, rect: Rect) {
        self.clip.push(rect);
        self.mask_rect = None;
    }

    /// Pops the most recent clip rectangle.
    pub fn pop_clip(&mut self) {
        self.clip.pop();
        self.mask_rect = None;
    }

    /// Number of active clip rectangles.
    #[must_use]
    pub fn clip_depth(&self) -> usize {
        self.clip.len()
    }

    fn effective_clip(&self) -> Option<Rect> {
        if self.clip.is_empty() {
            return None;
        }
        let mut left = 0.0_f32;
        let mut top = 0.0_f32;
        let mut right = self.width() as f32 / self.scale;
        let mut bottom = self.height() as f32 / self.scale;
        for rect in &self.clip {
            left = left.max(rect.x);
            top = top.max(rect.y);
            right = right.min(rect.x + rect.w);
            bottom = bottom.min(rect.y + rect.h);
        }
        Some(Rect::new(
            left,
            top,
            (right - left).max(0.0),
            (bottom - top).max(0.0),
        ))
    }

    /// The effective clip in physical pixels, `(x0, y0, x1, y1)` exclusive.
    fn physical_clip(&self) -> (i32, i32, i32, i32) {
        match self.effective_clip() {
            Some(clip) => (
                (clip.x * self.scale).floor() as i32,
                (clip.y * self.scale).floor() as i32,
                ((clip.x + clip.w) * self.scale).ceil() as i32,
                ((clip.y + clip.h) * self.scale).ceil() as i32,
            ),
            None => (0, 0, self.width() as i32, self.height() as i32),
        }
    }

    fn sync_mask(&mut self) {
        let Some(clip) = self.effective_clip() else {
            self.mask_rect = None;
            return;
        };
        let physical = Rect::new(
            (clip.x * self.scale).floor(),
            (clip.y * self.scale).floor(),
            (clip.w * self.scale).ceil(),
            (clip.h * self.scale).ceil(),
        );
        if self.mask_rect == Some(physical) && self.mask.is_some() {
            return;
        }
        self.mask_builds += 1;
        let mut mask = self.mask.take().unwrap_or_else(|| {
            Mask::new(self.width(), self.height()).expect("canvas is non-empty")
        });
        mask.clear();
        if let Some(rect) =
            tiny_skia::Rect::from_xywh(physical.x, physical.y, physical.w, physical.h)
        {
            let path = PathBuilder::from_rect(rect);
            mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
        }
        self.mask = Some(mask);
        self.mask_rect = Some(physical);
    }

    /// Takes the mask out so it can be passed to `tiny-skia` while the pixmap is
    /// borrowed mutably.
    fn take_mask(&mut self) -> Option<Mask> {
        self.mask.take()
    }

    fn restore_mask(&mut self, mask: Option<Mask>) {
        self.mask = mask;
    }

    /// Sets the logical-to-physical scale.
    ///
    /// The canvas size stays in physical pixels; every logical coordinate is
    /// multiplied by `scale`. A 2x display therefore gets 2x detail without the
    /// layout engine knowing anything about density.
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = if scale > 0.0 { scale } else { 1.0 };
    }

    /// Current logical-to-physical scale.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    fn logical_transform(&self) -> Transform {
        Transform::from_scale(self.scale, self.scale)
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

    /// How many coverage masks have been built.
    ///
    /// A shape fully inside the clip draws without one, so this is the testable
    /// form of "clipping is free when nothing is cut".
    #[must_use]
    pub const fn mask_builds(&self) -> u64 {
        self.mask_builds
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
    ///
    /// Axis-aligned fills go straight to pixels: no path, no mask. This is the
    /// operation behind every row background in a table, so it is the single
    /// most important fast path in the rasteriser.
    pub fn fill_rect(&mut self, rect: Rect, paint: Paint) {
        if paint.color.a() == 0 {
            return;
        }
        let scale = self.scale;
        let x0 = (rect.x * scale).round() as i32;
        let y0 = (rect.y * scale).round() as i32;
        let x1 = ((rect.x + rect.w) * scale).round() as i32;
        let y1 = ((rect.y + rect.h) * scale).round() as i32;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let clip = self.physical_clip();
        let width = self.width() as usize;
        pixel::fill_rect(
            self.pixmap.data_mut(),
            width,
            clip,
            (x0, y0, x1, y1),
            paint.color,
        );
    }

    /// Fills a rounded rectangle.
    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, paint: Paint) {
        let Some(path) = rect_path(rect, radius) else {
            return;
        };
        self.fill_path_in(&path, paint, rect);
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
        self.stroke_path_in(path, skia, stroke, rect, width * 0.5);
    }

    /// Fills a circle.
    pub fn fill_circle(&mut self, center: Point, radius: f32, paint: Paint) {
        if radius <= 0.0 {
            return;
        }
        let Some(path) = ellipse_path(center, radius, radius) else {
            return;
        };
        let bbox = Rect::new(
            center.x - radius,
            center.y - radius,
            radius * 2.0,
            radius * 2.0,
        );
        self.fill_path_in(&path, paint, bbox);
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
        let pad = width * 0.5 + 0.5;
        let bbox = Rect::new(
            from.x.min(to.x) - pad,
            from.y.min(to.y) - pad,
            (from.x - to.x).abs() + pad * 2.0,
            (from.y - to.y).abs() + pad * 2.0,
        );
        self.stroke_path_in(path, skia, stroke, bbox, 0.0);
    }

    /// Fills an arbitrary path built by the caller.
    pub fn fill_path(&mut self, path: &Path, paint: Paint) {
        let bounds = path.bounds();
        let bbox = Rect::new(bounds.x(), bounds.y(), bounds.width(), bounds.height());
        self.fill_path_in(path, paint, bbox);
    }

    /// Fills a path whose logical bounds are known.
    ///
    /// Fully visible paths draw straight into the pixmap; only a path that
    /// crosses the clip edge samples the coverage mask. That distinction is
    /// what keeps a scrolled list from turning every row into a mask blend.
    fn fill_path_in(&mut self, path: &Path, paint: Paint, bbox: Rect) {
        let clip = self.effective_clip();
        if outside(clip, bbox) {
            return;
        }
        let skia = to_skia_paint(paint);
        let transform = self.logical_transform();
        if clip.is_none() || inside(clip, bbox) {
            self.pixmap
                .fill_path(path, &skia, FillRule::Winding, transform, None);
            return;
        }
        self.sync_mask();
        let mask = self.take_mask();
        self.pixmap
            .fill_path(path, &skia, FillRule::Winding, transform, mask.as_ref());
        self.restore_mask(mask);
    }

    fn stroke_path_in(
        &mut self,
        path: Path,
        skia: SkiaPaint,
        stroke: tiny_skia::Stroke,
        bbox: Rect,
        grow: f32,
    ) {
        let bbox = Rect::new(
            bbox.x - grow,
            bbox.y - grow,
            bbox.w + grow * 2.0,
            bbox.h + grow * 2.0,
        );
        let clip = self.effective_clip();
        if outside(clip, bbox) {
            return;
        }
        let transform = self.logical_transform();
        if clip.is_none() || inside(clip, bbox) {
            self.pixmap
                .stroke_path(&path, &skia, &stroke, transform, None);
            return;
        }
        self.sync_mask();
        let mask = self.take_mask();
        self.pixmap
            .stroke_path(&path, &skia, &stroke, transform, mask.as_ref());
        self.restore_mask(mask);
    }

    /// Draws shaped text at `origin`, where `origin.y` is the top of the block.
    ///
    /// Each glyph is rasterised once per (font, glyph, size, device scale,
    /// quarter-pixel phase) into a coverage bitmap, then blended as a span.
    /// Repeating the same row a thousand times costs memory bandwidth, not
    /// outline conversion.
    pub fn draw_text(
        &mut self,
        engine: &TextEngine,
        shaped: &ShapedText,
        size: f32,
        origin: Point,
        color: Rgba8,
    ) {
        if color.a() == 0 || shaped.glyphs.is_empty() {
            return;
        }
        let scale = self.scale;
        let size_dev = size * scale;
        let clip = self.physical_clip();
        let stride = self.width() as usize;
        let scale_key = (scale * 64.0).round() as u32;
        let data = self.pixmap.data_mut();

        for glyph in &shaped.glyphs {
            let pen_x = (origin.x + glyph.x + glyph.x_offset) * scale;
            let pen_y = (origin.y + glyph.y - glyph.y_offset) * scale;
            let fraction = pen_x - pen_x.floor();
            let phase = ((fraction * 4.0).round() as i32).rem_euclid(4) as u8;
            let phase_offset = f32::from(phase) / 4.0;

            let key = GlyphKey {
                font: glyph.font.0,
                glyph: glyph.glyph_id,
                size: (size_dev * 64.0).round().max(0.0) as u32,
                scale: scale_key,
                phase,
            };
            let bitmap = self.glyphs.entry(key).or_insert_with(|| {
                let path = glyph_path(engine, glyph.font, glyph.glyph_id, size_dev)?;
                glyph::rasterize(&path, phase_offset)
            });
            let Some(bitmap) = bitmap.as_ref() else {
                continue;
            };

            let x = (pen_x + bitmap.left - phase_offset).round() as i32;
            let y = (pen_y + bitmap.top).round() as i32;
            if x + bitmap.width as i32 <= clip.0
                || x >= clip.2
                || y + bitmap.height as i32 <= clip.1
                || y >= clip.3
            {
                continue;
            }

            pixel::blit_alpha(
                data,
                stride,
                clip,
                (x, y),
                (bitmap.width, bitmap.height),
                &bitmap.alpha,
                color,
            );
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

fn inside(clip: Option<Rect>, bbox: Rect) -> bool {
    match clip {
        Some(clip) => {
            bbox.x >= clip.x - 0.01
                && bbox.y >= clip.y - 0.01
                && bbox.x + bbox.w <= clip.x + clip.w + 0.01
                && bbox.y + bbox.h <= clip.y + clip.h + 0.01
        }
        None => true,
    }
}

fn outside(clip: Option<Rect>, bbox: Rect) -> bool {
    match clip {
        Some(clip) => {
            bbox.w <= 0.0
                || bbox.h <= 0.0
                || bbox.x + bbox.w <= clip.x
                || bbox.x >= clip.x + clip.w
                || bbox.y + bbox.h <= clip.y
                || bbox.y >= clip.y + clip.h
        }
        None => false,
    }
}
