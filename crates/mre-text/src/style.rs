//! Text style.

/// Horizontal alignment of a text block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Flush left (or right in an RTL paragraph).
    #[default]
    Start,
    /// Centred.
    Center,
    /// Flush right (or left in an RTL paragraph).
    End,
}

/// How a paragraph breaks when it does not fit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Wrap {
    /// Never wrap; the text overflows.
    None,
    /// Wrap at word boundaries, falling back to character boundaries for a word
    /// longer than the line.
    #[default]
    Word,
}

/// Everything needed to shape and lay out a run of text.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Em size in logical pixels.
    pub size: f32,
    /// Weight, 100..=900. Fonts are matched to the nearest available weight.
    pub weight: u16,
    /// Italic / oblique.
    pub italic: bool,
    /// Preferred family. `None` means "first registered font".
    pub family: Option<String>,
    /// Line height as a multiple of the font size. `None` derives it from the
    /// font's own metrics, which is what avoids clipped ascenders.
    pub line_height: Option<f32>,
    /// Extra space between clusters, in logical pixels.
    pub letter_spacing: f32,
    /// Horizontal alignment within the available width.
    pub align: TextAlign,
    /// Wrapping mode.
    pub wrap: Wrap,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 14.0,
            weight: 400,
            italic: false,
            family: None,
            line_height: None,
            letter_spacing: 0.0,
            align: TextAlign::Start,
            wrap: Wrap::Word,
        }
    }
}

impl TextStyle {
    /// A style at the given size.
    #[must_use]
    pub fn new(size: f32) -> Self {
        Self {
            size,
            ..Self::default()
        }
    }

    /// Sets the family.
    #[must_use]
    pub fn family(mut self, family: impl Into<String>) -> Self {
        self.family = Some(family.into());
        self
    }

    /// Sets the weight.
    #[must_use]
    pub fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    /// Sets the line height multiplier.
    #[must_use]
    pub fn line_height(mut self, multiplier: f32) -> Self {
        self.line_height = Some(multiplier);
        self
    }

    /// Sets alignment.
    #[must_use]
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Sets wrapping.
    #[must_use]
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = wrap;
        self
    }

    /// Sets letter spacing.
    #[must_use]
    pub fn letter_spacing(mut self, spacing: f32) -> Self {
        self.letter_spacing = spacing;
        self
    }
}
