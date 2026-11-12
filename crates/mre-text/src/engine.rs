//! Font loading, shaping and line layout.
//!
//! Shaping is HarfBuzz (via `rustybuzz`) with font fallback: text is split into
//! runs that share a font, each run is shaped once, and the results are cached
//! by `(text, style, width)`. A table that scrolls through 50k rows shapes each
//! visible row exactly once, then reuses the result.
//!
//! Line breaking uses the Unicode line-breaking algorithm, so wrapping is
//! correct for CJK and for languages with no spaces, not just for Latin text.

use crate::shape::{Glyph, ShapedText, TextLine};
use crate::style::{TextAlign, TextStyle, Wrap};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// A handle to a registered font face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub u32);

impl FontId {
    /// Index into the engine's font list.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// Describes a font for matching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontDesc {
    /// Family name, as reported by the font.
    pub family: String,
    /// Weight, 100..=900.
    pub weight: u16,
    /// Italic or oblique.
    pub italic: bool,
}

impl FontDesc {
    /// A regular-weight description.
    #[must_use]
    pub fn new(family: impl Into<String>) -> Self {
        Self {
            family: family.into(),
            weight: 400,
            italic: false,
        }
    }
}

struct FontEntry {
    data: Arc<Vec<u8>>,
    index: u32,
    desc: FontDesc,
    upem: f32,
    ascender: f32,
    descender: f32,
    line_gap: f32,
    /// The height of a capital letter, in font units.
    ///
    /// This is what the eye judges when it says whether a line of text is
    /// centred: the line box is not the same shape as the ink on the page.
    cap_height: f32,
    /// The height of a lower case letter, in font units.
    x_height: f32,
}

/// Shaping counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Paragraphs shaped.
    pub shapes: u64,
    /// Lookups served from the shaping cache.
    pub cache_hits: u64,
}

/// Fonts, shaping and the shaped-run cache.
pub struct TextEngine {
    fonts: Vec<FontEntry>,
    cache: HashMap<u64, Arc<ShapedText>>,
    stats: Stats,
}

/// Upper bound on cached shaped runs. A long session with dynamic text should
/// not grow without bound, and re-shaping is far cheaper than an OOM.
const CACHE_LIMIT: usize = 4096;

/// How much wider than the limit a line may be before it is broken.
///
/// This is a rounding tolerance, not a margin: it exists so that a text
/// measured at its own width is not broken by a float ULP.
const WRAP_EPSILON: f32 = 0.01;

impl Default for TextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEngine {
    /// An engine with no fonts. Shaping returns empty output until a font is
    /// added, which is a loud failure rather than a silent fallback.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fonts: Vec::new(),
            cache: HashMap::new(),
            stats: Stats::default(),
        }
    }

    /// Number of registered faces.
    #[must_use]
    pub fn font_count(&self) -> usize {
        self.fonts.len()
    }

    /// Counters.
    #[must_use]
    pub const fn stats(&self) -> Stats {
        self.stats
    }

    /// Registers a font from memory.
    ///
    /// Returns `None` when the data is not a font or `index` is out of range.
    pub fn add_font(&mut self, data: Vec<u8>, index: u32, desc: FontDesc) -> Option<FontId> {
        let (upem, ascender, descender, line_gap, cap_height, x_height) = {
            let face = rustybuzz::Face::from_slice(&data, index)?;
            (
                (face.units_per_em() as f32).max(1.0),
                face.ascender() as f32,
                face.descender() as f32,
                face.line_gap() as f32,
                face.capital_height().unwrap_or_else(|| face.ascender()) as f32,
                face.x_height().unwrap_or_else(|| face.ascender()) as f32,
            )
        };
        let id = FontId(u32::try_from(self.fonts.len()).ok()?);
        self.fonts.push(FontEntry {
            data: Arc::new(data),
            index,
            desc,
            upem,
            ascender,
            descender,
            line_gap,
            cap_height,
            x_height,
        });
        self.cache.clear();
        Some(id)
    }

    /// Registers a font from a file.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if the file cannot be read, or an error if the
    /// bytes are not a usable font.
    pub fn add_font_file(
        &mut self,
        path: impl AsRef<Path>,
        desc: FontDesc,
    ) -> std::io::Result<FontId> {
        let data = std::fs::read(path)?;
        self.add_font(data, 0, desc).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "not a usable font")
        })
    }

    /// Registers a family from memory, guessing weight and italic from the face.
    pub fn add_font_auto(&mut self, data: Vec<u8>, index: u32) -> Option<FontId> {
        let face = rustybuzz::Face::from_slice(&data, index)?;
        let family = face
            .names()
            .into_iter()
            .find(|name| name.name_id == 1) // name ID 1 is the family name
            .and_then(|name| name.to_string())
            .unwrap_or_else(|| "Unknown".to_string());
        let weight = face.weight().to_number();
        let italic = face.is_italic();
        self.add_font(
            data,
            index,
            FontDesc {
                family,
                weight,
                italic,
            },
        )
    }

    /// Registers the platform's default sans-serif, serif and monospace faces.
    ///
    /// Capped at a small number of faces: loading every system font costs tens
    /// of megabytes and milliseconds of startup, which is exactly the kind of
    /// cost the budget forbids.
    ///
    /// Returns the number of faces added.
    pub fn load_system(&mut self) -> usize {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        let mut wanted: Vec<fontdb::ID> = Vec::new();
        // The CSS generic families have to be queried through fontdb's own
        // variants: `Family::Name("sans-serif")` matches no font, because no
        // typeface is literally named "sans-serif". fontdb resolves the
        // variants through its own classification of installed faces.
        for family in [
            fontdb::Family::SansSerif,
            fontdb::Family::Serif,
            fontdb::Family::Monospace,
        ] {
            for weight in [fontdb::Weight::NORMAL, fontdb::Weight::BOLD] {
                let query = fontdb::Query {
                    families: &[family],
                    weight,
                    style: fontdb::Style::Normal,
                    stretch: fontdb::Stretch::Normal,
                };
                if let Some(id) = db.query(&query) {
                    if !wanted.contains(&id) {
                        wanted.push(id);
                    }
                }
            }
        }
        if wanted.is_empty() {
            // No generic families resolved (an unusual font set): fall back to
            // the first faces installed, because any font beats none — text
            // shapes to nothing without one.
            wanted.extend(db.faces().map(|face| face.id).take(12));
        }
        wanted.truncate(12);

        let mut added = 0;
        for id in wanted {
            let mut payload = None;
            db.with_face_data(id, |data, index| {
                payload = Some((data.to_vec(), index));
            });
            if let Some((data, index)) = payload {
                if self.add_font_auto(data, index).is_some() {
                    added += 1;
                }
            }
        }
        added
    }

    /// Shapes and lays out `text`.
    ///
    /// `max_width` is the wrap width in logical pixels; pass `f32::INFINITY` for
    /// no wrapping.
    pub fn shape(&mut self, text: &str, style: &TextStyle, max_width: f32) -> Arc<ShapedText> {
        let key = cache_key(text, style, max_width);
        if let Some(shaped) = self.cache.get(&key) {
            self.stats.cache_hits += 1;
            return Arc::clone(shaped);
        }

        self.stats.shapes += 1;
        let shaped = Arc::new(self.shape_uncached(text, style, max_width));
        if self.cache.len() >= CACHE_LIMIT {
            self.cache.clear();
        }
        self.cache.insert(key, Arc::clone(&shaped));
        shaped
    }

    /// Drops every cached shaped run.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Units per em for a font, used to scale glyph outlines.
    #[must_use]
    pub fn units_per_em(&self, font: FontId) -> f32 {
        self.fonts
            .get(font.index())
            .map_or(1000.0, |entry| entry.upem)
    }

    /// Runs `f` with a font's raw bytes and face index.
    ///
    /// This is how the rasteriser reaches glyph outlines without the text crate
    /// having to depend on a rasteriser.
    pub fn with_font_data<R>(&self, font: FontId, f: impl FnOnce(&[u8], u32) -> R) -> Option<R> {
        let entry = self.fonts.get(font.index())?;
        Some(f(entry.data.as_slice(), entry.index))
    }

    /// Family name of a registered font.
    #[must_use]
    pub fn family(&self, font: FontId) -> Option<&str> {
        self.fonts
            .get(font.index())
            .map(|entry| entry.desc.family.as_str())
    }

    fn primary(&self, style: &TextStyle) -> Option<usize> {
        self.candidates(style).first().copied()
    }

    /// Fonts in preference order for a style.
    fn candidates(&self, style: &TextStyle) -> Vec<usize> {
        let mut indices: Vec<usize> = (0..self.fonts.len()).collect();
        indices.sort_by_key(|&index| {
            let entry = &self.fonts[index];
            let family_penalty = match &style.family {
                Some(wanted) if entry.desc.family.eq_ignore_ascii_case(wanted) => 0,
                Some(_) => 1,
                None => 0,
            };
            let italic_penalty = u32::from(entry.desc.italic != style.italic);
            let weight_penalty = entry.desc.weight.abs_diff(style.weight);
            (family_penalty, italic_penalty, weight_penalty)
        });
        indices
    }

    /// The cap height, the x-height, the ascender and the descender, at this
    /// style's size.
    fn extents(&self, style: &TextStyle) -> (f32, f32, f32, f32) {
        let Some(primary) = self.primary(style) else {
            let size = style.size;
            return (size * 0.7, size * 0.5, size * 0.8, size * 0.2);
        };
        let entry = &self.fonts[primary];
        let scale = style.size / entry.upem;
        (
            entry.cap_height * scale,
            entry.x_height * scale,
            entry.ascender * scale,
            -entry.descender * scale,
        )
    }

    /// The height of a capital letter at this style's size.
    ///
    /// This is the band the eye uses to judge whether a line of text sits in
    /// the middle of a box.
    #[must_use]
    pub fn cap_height(&self, style: &TextStyle) -> f32 {
        let Some(primary) = self.primary(style) else {
            return style.size * 0.7;
        };
        let entry = &self.fonts[primary];
        entry.cap_height * (style.size / entry.upem)
    }

    /// The height of a lower case letter at this style's size.
    #[must_use]
    pub fn x_height(&self, style: &TextStyle) -> f32 {
        let Some(primary) = self.primary(style) else {
            return style.size * 0.5;
        };
        let entry = &self.fonts[primary];
        entry.x_height * (style.size / entry.upem)
    }

    /// How far a line of text has to move down to be optically centred.
    ///
    /// A line box is taller than the ink on the page: the space below the
    /// baseline is empty, so centring the box leaves the letters high. This
    /// is the distance between the middle of the line box and the middle of
    /// the letters.
    ///
    /// The eye judges a line of lower case by its x-height and a line of
    /// capitals by its cap height, so the reference follows the text.
    #[must_use]
    pub fn optical_offset(&self, text: &str, style: &TextStyle, height: f32, baseline: f32) -> f32 {
        let (cap, x, _, _) = self.extents(style);
        let reference = reference_band(text, cap, x);
        (height / 2.0 - baseline + reference / 2.0).clamp(-height, height)
    }

    /// The ink of a run: how far it reaches above and below the baseline.
    #[must_use]
    pub fn ink(&self, text: &str, style: &TextStyle) -> (f32, f32) {
        let (cap, x, ascender, descender) = self.extents(style);
        let mut top = 0.0_f32;
        let mut bottom = 0.0_f32;
        for ch in text.chars() {
            let (up, down) = ink_extent(ch, cap, x, ascender, descender);
            top = top.max(up);
            bottom = bottom.max(down);
        }
        (top, bottom)
    }

    /// The space above the ink of a run and the space below it, relative to
    /// the run's box.
    ///
    /// A line box is taller than the letters: there is room above the caps
    /// and below the descenders that nothing is drawn in. A gap between two
    /// blocks of text is a gap between the letters, so these are what has to
    /// be taken off it.
    #[must_use]
    pub fn ink_spaces(&self, text: &str, style: &TextStyle) -> (f32, f32) {
        let (baseline, height) = self.line_metrics(style);
        let (top, bottom) = self.ink(text, style);
        let visual = baseline + self.optical_offset(text, style, height, baseline);
        (visual - top, height - visual - bottom)
    }

    /// The visual baseline of a run, relative to the top of its box.
    ///
    /// This is the baseline the letters actually sit on, after the optical
    /// centring: text that sits side by side is aligned on this, not on the
    /// font's own baseline.
    #[must_use]
    pub fn visual_baseline(&self, text: &str, style: &TextStyle) -> f32 {
        let (baseline, height) = self.line_metrics(style);
        baseline + self.optical_offset(text, style, height, baseline).abs()
    }

    fn line_metrics(&self, style: &TextStyle) -> (f32, f32) {
        let Some(primary) = self.primary(style) else {
            let height = style.size * 1.2;
            return (style.size, height);
        };
        let entry = &self.fonts[primary];
        let scale = style.size / entry.upem;
        let ascent = entry.ascender * scale;
        let descent = -entry.descender * scale;
        let natural = (ascent + descent + entry.line_gap * scale).max(style.size);
        let height = style
            .line_height
            .map_or(natural.max(style.size * 1.1), |multiplier| {
                (multiplier * style.size).max(style.size)
            });
        let baseline = (height - natural) / 2.0 + ascent;
        (baseline, height)
    }

    fn shape_uncached(&self, text: &str, style: &TextStyle, max_width: f32) -> ShapedText {
        let (font_baseline, font_height) = self.line_metrics(style);
        // The letters are optically centred, which moves them down within the
        // line box. The box grows to hold them, so the ink never leaves it.
        let inset = self
            .optical_offset(text, style, font_height, font_baseline)
            .abs();
        let baseline = font_baseline + inset;
        let line_height = font_height + inset;

        if text.is_empty() {
            return ShapedText {
                lines: vec![TextLine {
                    start: 0,
                    end: 0,
                    width: 0.0,
                    baseline,
                    y: 0.0,
                    height: line_height,
                    glyph_start: 0,
                    glyph_end: 0,
                }],
                height: line_height,
                ..ShapedText::default()
            };
        }

        let rtl = paragraph_is_rtl(text);
        let candidates = self.candidates(style);

        // Faces are created once per shape call and borrow the font data, which
        // lives in `self` for at least as long as this function.
        let faces: Vec<(FontId, Option<rustybuzz::Face<'_>>, &FontEntry)> = candidates
            .iter()
            .map(|&index| {
                let entry = &self.fonts[index];
                let face = rustybuzz::Face::from_slice(entry.data.as_slice(), entry.index);
                (FontId(index as u32), face, entry)
            })
            .collect();

        let mut shaped = ShapedText {
            rtl,
            ..ShapedText::default()
        };
        let mut y = 0.0_f32;
        let mut fallbacks = 0_u32;
        let mut offset = 0_usize;

        for paragraph in text.split('\n') {
            let paragraph_len = paragraph.len();
            let mut glyphs = Vec::new();
            let mut breaks: std::collections::HashSet<u32> = std::collections::HashSet::new();

            if !paragraph.is_empty() {
                let runs = split_runs(paragraph, &faces);
                fallbacks += runs.fallback_runs;

                let order: Vec<&FontRun> = if rtl {
                    runs.runs.iter().rev().collect()
                } else {
                    runs.runs.iter().collect()
                };

                for run in order {
                    if run.font >= faces.len() {
                        continue;
                    }
                    let (_, face, entry) = &faces[run.font];
                    let Some(face) = face else { continue };
                    let slice = &paragraph[run.start..run.end];
                    if slice.is_empty() {
                        continue;
                    }

                    let mut buffer = rustybuzz::UnicodeBuffer::new();
                    buffer.push_str(slice);
                    buffer.set_direction(if rtl {
                        rustybuzz::Direction::RightToLeft
                    } else {
                        rustybuzz::Direction::LeftToRight
                    });

                    let output = rustybuzz::shape(face, &[], buffer);
                    let scale = style.size / entry.upem;

                    for (info, position) in
                        output.glyph_infos().iter().zip(output.glyph_positions())
                    {
                        glyphs.push(Glyph {
                            font: faces[run.font].0,
                            glyph_id: u16::try_from(info.glyph_id).unwrap_or(0),
                            x: 0.0,
                            y: 0.0,
                            advance: position.x_advance as f32 * scale + style.letter_spacing,
                            cluster: (offset + run.start + info.cluster as usize) as u32,
                            x_offset: position.x_offset as f32 * scale,
                            y_offset: position.y_offset as f32 * scale,
                        });
                    }
                }

                if style.wrap == Wrap::Word && !rtl {
                    for (byte, opportunity) in unicode_linebreak::linebreaks(paragraph) {
                        if opportunity == unicode_linebreak::BreakOpportunity::Allowed {
                            breaks.insert((offset + byte) as u32);
                        }
                    }
                }
            }

            let glyph_start = shaped.glyphs.len() as u32;
            let wrap_width = if style.wrap == Wrap::Word {
                max_width
            } else {
                f32::INFINITY
            };
            let mut line_ranges = wrap(&glyphs, wrap_width, &breaks);

            if rtl && line_ranges.len() > 1 {
                // RTL wrapping needs a bidi reorder per line. Until that lands,
                // keep RTL paragraphs on one line rather than misordering them.
                line_ranges = vec![(0, glyphs.len())];
            }

            let cumulative = cumulative_advances(&glyphs);

            for (start, end) in line_ranges {
                let line_width = cumulative[end] - cumulative[start];
                let available = if max_width.is_finite() {
                    max_width
                } else {
                    line_width
                };
                let align = if rtl {
                    match style.align {
                        TextAlign::Start => TextAlign::End,
                        TextAlign::End => TextAlign::Start,
                        other => other,
                    }
                } else {
                    style.align
                };
                let x_offset = match align {
                    TextAlign::Start => 0.0,
                    TextAlign::Center => (available - line_width) / 2.0,
                    TextAlign::End => available - line_width,
                };

                for index in start..end {
                    let mut glyph = glyphs[index];
                    glyph.x = cumulative[index] - cumulative[start] + x_offset;
                    glyph.y = y + baseline;
                    shaped.glyphs.push(glyph);
                }

                shaped.lines.push(TextLine {
                    start: glyphs.get(start).map_or(0, |glyph| glyph.cluster),
                    end: glyphs
                        .get(end.saturating_sub(1))
                        .map_or(0, |glyph| glyph.cluster),
                    width: line_width,
                    baseline: y + baseline,
                    y,
                    height: line_height,
                    glyph_start,
                    glyph_end: shaped.glyphs.len() as u32,
                });
                shaped.width = shaped.width.max(line_width);
                y += line_height;
            }

            offset += paragraph_len + 1;
        }

        shaped.height = y.max(line_height);
        shaped.fallbacks = fallbacks;
        shaped
    }
}

struct FontRun {
    font: usize,
    start: usize,
    end: usize,
}

struct Runs {
    runs: Vec<FontRun>,
    fallback_runs: u32,
}

fn split_runs(
    paragraph: &str,
    faces: &[(FontId, Option<rustybuzz::Face<'_>>, &FontEntry)],
) -> Runs {
    let mut runs: Vec<FontRun> = Vec::new();
    let mut current: Option<(usize, usize)> = None;
    let mut fallback_runs = 0_u32;

    for (byte, ch) in paragraph.char_indices() {
        let font = font_for_char(ch, faces);
        match current {
            Some((active, start)) if active == font => {
                let _ = start;
            }
            Some((active, start)) => {
                runs.push(FontRun {
                    font: active,
                    start,
                    end: byte,
                });
                if active > 0 {
                    fallback_runs += 1;
                }
                current = Some((font, byte));
            }
            None => current = Some((font, byte)),
        }
    }

    if let Some((active, start)) = current {
        runs.push(FontRun {
            font: active,
            start,
            end: paragraph.len(),
        });
        if active > 0 {
            fallback_runs += 1;
        }
    }

    Runs {
        runs,
        fallback_runs,
    }
}

fn font_for_char(ch: char, faces: &[(FontId, Option<rustybuzz::Face<'_>>, &FontEntry)]) -> usize {
    for (index, (_, face, _)) in faces.iter().enumerate() {
        let Some(face) = face else { continue };
        if face.glyph_index(ch).is_some() {
            return index;
        }
    }
    0
}

fn paragraph_is_rtl(text: &str) -> bool {
    for ch in text.chars() {
        if ch.is_alphabetic() {
            return matches!(
                unicode_bidi::bidi_class(ch),
                unicode_bidi::BidiClass::R
                    | unicode_bidi::BidiClass::AL
                    | unicode_bidi::BidiClass::AN
            );
        }
    }
    false
}

/// How far the ink of a character reaches above and below the baseline.
///
/// Capitals reach the cap height, ascenders the ascender, most lower case
/// letters the x-height, and the descenders hang below the baseline.
fn ink_extent(ch: char, cap: f32, x: f32, ascender: f32, descender: f32) -> (f32, f32) {
    if ch.is_whitespace() {
        return (0.0, 0.0);
    }
    if ch.is_uppercase() || ch.is_ascii_digit() {
        return (cap, 0.0);
    }
    match ch {
        'g' | 'j' | 'p' | 'q' | 'y' => (x, descender),
        'b' | 'd' | 'f' | 'h' | 'k' | 'l' | 't' | 'i' => (ascender, 0.0),
        _ => (x, 0.0),
    }
}

/// The band a line of text is judged by when it is centred.
///
/// The eye does not centre the line box: it centres the mass of the
/// letters. A line of capitals is judged by its cap height, a line of
/// lower case by its x-height, and a line that mixes them by whichever
/// dominates — the ascenders and descenders are few and thin, so they do
/// not carry the weight.
fn reference_band(text: &str, cap: f32, x: f32) -> f32 {
    let mut capitals = 0_usize;
    let mut lower = 0_usize;
    for ch in text.chars() {
        if ch.is_uppercase() {
            capitals += 1;
        } else if matches!(
            ch,
            'a' | 'c' | 'e' | 'm' | 'n' | 'o' | 'r' | 's' | 'u' | 'v' | 'w' | 'x' | 'z'
        ) {
            lower += 1;
        }
    }
    if capitals > lower { cap } else { x }
}

fn cumulative_advances(glyphs: &[Glyph]) -> Vec<f32> {
    let mut cumulative = Vec::with_capacity(glyphs.len() + 1);
    cumulative.push(0.0);
    let mut total = 0.0;
    for glyph in glyphs {
        total += glyph.advance;
        cumulative.push(total);
    }
    cumulative
}

/// Greedy line breaking at Unicode break opportunities.
///
/// A line is broken only when it is *meaningfully* wider than the limit.
/// The layout re-measures a text at the width it has just measured it at, and
/// the two can differ by a float ULP; a strict comparison would then break a
/// word that fits, and a label would wrap onto a second line for no reason.
fn wrap(
    glyphs: &[Glyph],
    max_width: f32,
    breaks: &std::collections::HashSet<u32>,
) -> Vec<(usize, usize)> {
    if glyphs.is_empty() {
        return vec![(0, 0)];
    }

    let cumulative = cumulative_advances(glyphs);
    if !max_width.is_finite() || max_width <= 0.0 {
        return vec![(0, glyphs.len())];
    }

    let mut lines = Vec::new();
    let mut start = 0_usize;
    let mut last_break: Option<usize> = None;
    let mut index = 0_usize;

    while index < glyphs.len() {
        if breaks.contains(&glyphs[index].cluster) && index > start {
            last_break = Some(index);
        }

        let width = cumulative[index + 1] - cumulative[start];
        if width > max_width + WRAP_EPSILON && index > start {
            let end = last_break
                .filter(|candidate| *candidate > start && *candidate <= index)
                .unwrap_or(index);
            lines.push((start, end));
            start = end;
            last_break = None;
            continue;
        }
        index += 1;
    }

    lines.push((start, glyphs.len()));
    lines
}

fn cache_key(text: &str, style: &TextStyle, max_width: f32) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash ^= u64::from(style.size.to_bits());
    hash = hash.wrapping_mul(PRIME);
    hash ^= u64::from(style.weight);
    hash = hash.wrapping_mul(PRIME);
    hash ^= u64::from(style.italic);
    hash = hash.wrapping_mul(PRIME);
    hash ^= u64::from(style.letter_spacing.to_bits());
    hash = hash.wrapping_mul(PRIME);
    hash ^= match style.family {
        Some(ref family) => {
            let mut family_hash = 0_u64;
            for byte in family.as_bytes() {
                family_hash ^= u64::from(*byte);
                family_hash = family_hash.wrapping_mul(PRIME);
            }
            family_hash
        }
        None => 0,
    };
    hash = hash.wrapping_mul(PRIME);
    hash ^= match style.line_height {
        Some(multiplier) => u64::from(multiplier.to_bits()),
        None => u64::MAX,
    };
    hash = hash.wrapping_mul(PRIME);
    hash ^= match style.align {
        TextAlign::Start => 0,
        TextAlign::Center => 1,
        TextAlign::End => 2,
    };
    hash = hash.wrapping_mul(PRIME);
    hash ^= match style.wrap {
        Wrap::None => 0,
        Wrap::Word => 1,
    };
    hash = hash.wrapping_mul(PRIME);
    hash ^= u64::from(quantize(max_width));
    hash
}

fn quantize(value: f32) -> u32 {
    if !value.is_finite() {
        u32::MAX
    } else {
        (value.max(0.0) * 64.0).round().min(u32::MAX as f32) as u32
    }
}
