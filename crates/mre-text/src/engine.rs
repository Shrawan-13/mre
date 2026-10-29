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
        let (upem, ascender, descender, line_gap) = {
            let face = rustybuzz::Face::from_slice(&data, index)?;
            (
                (face.units_per_em() as f32).max(1.0),
                face.ascender() as f32,
                face.descender() as f32,
                face.line_gap() as f32,
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
        for family in ["sans-serif", "serif", "monospace"] {
            for weight in [fontdb::Weight::NORMAL, fontdb::Weight::BOLD] {
                let query = fontdb::Query {
                    families: &[fontdb::Family::Name(family)],
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
        let (baseline, line_height) = self.line_metrics(style);

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
        if width > max_width && index > start {
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
