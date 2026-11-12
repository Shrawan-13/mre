//! Glyph outlines, fitted to the pixel grid.
//!
//! An outline that is merely scaled lands wherever the typeface's design
//! puts it. At eleven pixels a stem can be one pixel wide in one letter and
//! split across two in the next, and a round letter can end a pixel below
//! the flat ones beside it, so the letters of a word stop looking like one
//! piece.
//!
//! Every engine that draws small text well fits the outline to the pixel
//! grid before drawing it: a browser on Linux asks FreeType for the light
//! automatic hinter, a browser on Windows asks DirectWrite for the font's
//! own hints, and Skia asks FreeType for the same. The hinter moves the
//! outline's points onto the grid — the x-height and the cap height land on
//! whole pixels — and the letter spacing is left alone, so the word keeps
//! its shape and every letter of it shares the same rows.
//!
//! The hinter is FreeType's, reimplemented in Rust: `skrifa` carries both
//! the TrueType interpreter and the automatic hinter, and the automatic one
//! is what a browser uses when the font has no hints worth trusting.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use mre_text::FontId;
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{
    DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};

/// One segment of a glyph outline.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Segment {
    /// Move to a point.
    MoveTo(f32, f32),
    /// A straight line.
    LineTo(f32, f32),
    /// A quadratic curve: control point, end point.
    QuadTo(f32, f32, f32, f32),
    /// A cubic curve: two control points, end point.
    CurveTo(f32, f32, f32, f32, f32, f32),
    /// Close the current contour.
    Close,
}

/// A glyph outline in device pixels, y down, with the baseline at y = 0.
pub(crate) struct GlyphOutline {
    /// The contour commands.
    pub(crate) segments: Vec<Segment>,
    /// The leftmost point.
    pub(crate) min_x: f32,
    /// The topmost point.
    pub(crate) min_y: f32,
    /// The rightmost point.
    pub(crate) max_x: f32,
    /// The bottommost point.
    pub(crate) max_y: f32,
}

impl GlyphOutline {
    /// An empty outline.
    fn new() -> Self {
        Self {
            segments: Vec::new(),
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    fn grow(&mut self, x: f32, y: f32) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }

    /// Scales the outline about the origin.
    pub(crate) fn scale(&mut self, factor: f32) {
        if factor == 1.0 {
            return;
        }
        for segment in &mut self.segments {
            *segment = match *segment {
                Segment::MoveTo(x, y) => Segment::MoveTo(x * factor, y * factor),
                Segment::LineTo(x, y) => Segment::LineTo(x * factor, y * factor),
                Segment::QuadTo(cx, cy, x, y) => {
                    Segment::QuadTo(cx * factor, cy * factor, x * factor, y * factor)
                }
                Segment::CurveTo(c1x, c1y, c2x, c2y, x, y) => Segment::CurveTo(
                    c1x * factor,
                    c1y * factor,
                    c2x * factor,
                    c2y * factor,
                    x * factor,
                    y * factor,
                ),
                Segment::Close => Segment::Close,
            };
        }
        self.min_x *= factor;
        self.max_x *= factor;
        self.min_y *= factor;
        self.max_y *= factor;
    }
}

/// The unhinted outline of a glyph, scaled to `scale` pixels a font unit.
///
/// This is the design's own shape: what the typeface says the letter is,
/// with nothing fitted to the grid.
pub(crate) fn unhinted(data: &[u8], index: u32, glyph: u16, scale: f32) -> Option<GlyphOutline> {
    let face = ttf_parser::Face::parse(data, index).ok()?;
    let mut sink = Sink {
        outline: GlyphOutline::new(),
        scale,
    };
    face.outline_glyph(ttf_parser::GlyphId(glyph), &mut sink)?;
    Some(sink.outline)
}

/// The hinted outline of a glyph, in device pixels.
///
/// The hinter has already fitted the shape to the grid at the instance's
/// size, so the outline is used as it comes.
pub(crate) fn hinted(
    data: &[u8],
    index: u32,
    glyph: u16,
    instance: &HintingInstance,
) -> Option<GlyphOutline> {
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let outlines = font.outline_glyphs();
    let glyph = outlines.get(skrifa::GlyphId::new(u32::from(glyph)))?;
    let mut pen = Pen {
        outline: GlyphOutline::new(),
    };
    glyph
        .draw(DrawSettings::hinted(instance, false), &mut pen)
        .ok()?;
    Some(pen.outline)
}

/// Collects a `ttf-parser` outline, scaled and with y pointing down.
struct Sink {
    outline: GlyphOutline,
    scale: f32,
}

impl Sink {
    fn sx(&self, x: f32) -> f32 {
        x * self.scale
    }

    fn sy(&self, y: f32) -> f32 {
        -y * self.scale
    }
}

impl ttf_parser::OutlineBuilder for Sink {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = (self.sx(x), self.sy(y));
        self.outline.segments.push(Segment::MoveTo(x, y));
        self.outline.grow(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = (self.sx(x), self.sy(y));
        self.outline.segments.push(Segment::LineTo(x, y));
        self.outline.grow(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (cx0, cy0) = (self.sx(cx0), self.sy(cy0));
        let (x, y) = (self.sx(x), self.sy(y));
        self.outline.segments.push(Segment::QuadTo(cx0, cy0, x, y));
        self.outline.grow(cx0, cy0);
        self.outline.grow(x, y);
    }

    fn curve_to(&mut self, cx1: f32, cy1: f32, cx2: f32, cy2: f32, x: f32, y: f32) {
        let (cx1, cy1) = (self.sx(cx1), self.sy(cy1));
        let (cx2, cy2) = (self.sx(cx2), self.sy(cy2));
        let (x, y) = (self.sx(x), self.sy(y));
        self.outline
            .segments
            .push(Segment::CurveTo(cx1, cy1, cx2, cy2, x, y));
        self.outline.grow(cx1, cy1);
        self.outline.grow(cx2, cy2);
        self.outline.grow(x, y);
    }

    fn close(&mut self) {
        self.outline.segments.push(Segment::Close);
    }
}

/// Collects a `skrifa` outline, with y pointing down.
struct Pen {
    outline: GlyphOutline,
}

impl Pen {
    fn grow(&mut self, x: f32, y: f32) {
        self.outline.grow(x, -y);
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.outline.segments.push(Segment::MoveTo(x, -y));
        self.grow(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.outline.segments.push(Segment::LineTo(x, -y));
        self.grow(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.outline
            .segments
            .push(Segment::QuadTo(cx0, -cy0, x, -y));
        self.grow(cx0, cy0);
        self.grow(x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.outline
            .segments
            .push(Segment::CurveTo(cx0, -cy0, cx1, -cy1, x, -y));
        self.grow(cx0, cy0);
        self.grow(cx1, cy1);
        self.grow(x, y);
    }

    fn close(&mut self) {
        self.outline.segments.push(Segment::Close);
    }
}

/// The hinting instances, one per face and size.
///
/// Building an instance is expensive: it reads the font's tables and works
/// out the hinter's style for every glyph. The instances are kept so that
/// the work is done once per face and size, not once per glyph.
#[derive(Default)]
pub(crate) struct Hinter {
    instances: HashMap<(u32, u32), HintingInstance>,
}

impl Hinter {
    /// The hinting instance for a face at `ppem` pixels.
    pub(crate) fn instance(
        &mut self,
        font: FontId,
        data: &[u8],
        index: u32,
        ppem: u32,
    ) -> Option<&HintingInstance> {
        let key = (font.0, ppem);
        match self.instances.entry(key) {
            Entry::Occupied(entry) => Some(entry.into_mut()),
            Entry::Vacant(entry) => {
                let face = skrifa::FontRef::from_index(data, index).ok()?;
                let outlines = face.outline_glyphs();
                let instance = HintingInstance::new(
                    &outlines,
                    Size::new(ppem as f32),
                    LocationRef::default(),
                    HintingOptions {
                        // The automatic hinter: it fits the heights of the
                        // letters to the grid and leaves their widths
                        // alone, which is what a browser asks for on Linux.
                        engine: Engine::Auto(None),
                        target: Target::Smooth {
                            mode: SmoothMode::Light,
                            // The two edges of a stem are drawn the same
                            // way, so a letter is not heavier on one side
                            // than the other.
                            symmetric_rendering: true,
                            // The spacing between the letters does not
                            // change with the size, so a word does not
                            // reflow when the hinter is on.
                            preserve_linear_metrics: true,
                        },
                    },
                )
                .ok()?;
                Some(entry.insert(instance))
            }
        }
    }
}

/// Whether the outlines of a face are fitted to the grid.
///
/// The hinter is what makes small text crisp, but it also bends the
/// design's proportions, and at twice the scale there are enough pixels
/// that the design is already crisp. So the hinter runs at one times the
/// scale and leaves the design alone above it.
pub(crate) fn should_hint(scale: f32) -> bool {
    scale <= 1.25
}
