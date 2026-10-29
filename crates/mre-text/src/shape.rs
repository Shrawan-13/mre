//! Shaped output: lines and positioned glyphs.

use crate::engine::FontId;
use mre_core::Size;

/// One positionable glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    /// Which font this glyph is drawn with.
    pub font: FontId,
    /// Glyph id inside that font.
    pub glyph_id: u16,
    /// Pen position within the text block, logical pixels.
    pub x: f32,
    /// Baseline of the line this glyph belongs to, logical pixels from the top
    /// of the text block.
    pub y: f32,
    /// Advance width, including letter spacing.
    pub advance: f32,
    /// Byte offset of the source cluster this glyph came from. Used for cursor
    /// placement and hit testing.
    pub cluster: u32,
    /// Pen offset applied before drawing (for marks and ligature components).
    pub x_offset: f32,
    /// Vertical pen offset, positive upwards.
    pub y_offset: f32,
}

/// One laid-out line.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    /// Byte offset of the first character on the line.
    pub start: u32,
    /// Byte offset just past the last character on the line.
    pub end: u32,
    /// Width of the line, excluding trailing whitespace collapse.
    pub width: f32,
    /// Baseline offset from the top of the text block.
    pub baseline: f32,
    /// Top of the line box.
    pub y: f32,
    /// Height of the line box.
    pub height: f32,
    /// Glyph range in [`ShapedText::glyphs`].
    pub glyph_start: u32,
    /// One past the last glyph.
    pub glyph_end: u32,
}

/// The result of shaping and laying out one string.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    /// All glyphs, in visual left-to-right order.
    pub glyphs: Vec<Glyph>,
    /// Line boxes, top to bottom.
    pub lines: Vec<TextLine>,
    /// Total width of the widest line.
    pub width: f32,
    /// Total height of the block.
    pub height: f32,
    /// True when the paragraph direction is right-to-left.
    pub rtl: bool,
    /// Number of font fallbacks used beyond the primary font. Exposed because a
    /// high number means the font set is missing coverage and text will look
    /// inconsistent.
    pub fallbacks: u32,
}

impl ShapedText {
    /// Size of the block.
    #[must_use]
    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// The line containing a byte offset.
    #[must_use]
    pub fn line_at(&self, byte: u32) -> Option<&TextLine> {
        self.lines
            .iter()
            .find(|line| byte >= line.start && byte < line.end)
            .or_else(|| self.lines.last())
    }

    /// The byte offset nearest to a horizontal position on a line. This is the
    /// primitive text selection and cursor placement are built on.
    #[must_use]
    pub fn index_at(&self, line: usize, x: f32) -> u32 {
        let Some(line) = self.lines.get(line) else {
            return 0;
        };
        let glyphs = &self.glyphs[line.glyph_start as usize..line.glyph_end as usize];
        for glyph in glyphs {
            if x < glyph.x + glyph.advance / 2.0 {
                return glyph.cluster;
            }
        }
        line.end
    }
}
