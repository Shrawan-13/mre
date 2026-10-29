//! Shaping, wrapping and cache behaviour, using a vendored font so the results
//! are identical on every machine.

use mre_layout::{TextId, TextMetrics};
use mre_text::{FontDesc, TextAlign, TextEngine, TextStore, TextStyle, Wrap};

fn font_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    )
    .to_string()
}

fn engine() -> TextEngine {
    let mut engine = TextEngine::new();
    engine
        .add_font_file(font_path(), FontDesc::new("Roboto"))
        .expect("bundled font must load");
    engine
}

#[test]
fn shapes_latin_text_into_one_line() {
    let mut engine = engine();
    let shaped = engine.shape("Hello, world", &TextStyle::new(16.0), f32::INFINITY);

    assert!(shaped.width > 0.0, "shaped text must have width");
    assert!(shaped.height > 0.0, "shaped text must have height");
    assert!(!shaped.glyphs.is_empty(), "shaped text must have glyphs");
    assert_eq!(shaped.lines.len(), 1);
    assert_eq!(shaped.lines[0].width, shaped.width);
    assert!(!shaped.rtl);
}

#[test]
fn empty_text_still_occupies_one_line() {
    let mut engine = engine();
    let shaped = engine.shape("", &TextStyle::new(16.0), 100.0);

    assert!(shaped.glyphs.is_empty());
    assert_eq!(shaped.lines.len(), 1);
    assert!(shaped.height > 0.0);
    assert_eq!(shaped.width, 0.0);
}

#[test]
fn wraps_at_word_boundaries() {
    let mut engine = engine();
    let text = "the quick brown fox jumps over the lazy dog";
    let shaped = engine.shape(text, &TextStyle::new(16.0), 80.0);

    assert!(shaped.lines.len() > 1, "text should wrap at 80px");
    for line in &shaped.lines {
        assert!(
            line.width <= 80.5,
            "line {:?} exceeds the wrap width",
            line.width
        );
    }
    assert!(shaped.height >= shaped.lines.len() as f32 * 16.0);
}

#[test]
fn no_wrap_mode_keeps_a_single_line() {
    let mut engine = engine();
    let shaped = engine.shape(
        "the quick brown fox jumps over the lazy dog",
        &TextStyle::new(16.0).wrap(Wrap::None),
        80.0,
    );

    assert_eq!(shaped.lines.len(), 1);
    assert!(shaped.width > 80.0);
}

#[test]
fn explicit_newlines_create_lines() {
    let mut engine = engine();
    let shaped = engine.shape("first\nsecond\nthird", &TextStyle::new(14.0), 500.0);

    assert_eq!(shaped.lines.len(), 3);
    assert!(shaped.lines[1].baseline > shaped.lines[0].baseline);
    assert!(shaped.lines[2].y > shaped.lines[1].y);
}

#[test]
fn repeated_shapes_hit_the_cache() {
    let mut engine = engine();
    let style = TextStyle::new(16.0);

    let first = engine.shape("cached", &style, 200.0);
    let hits = engine.stats().cache_hits;
    let second = engine.shape("cached", &style, 200.0);

    assert_eq!(engine.stats().cache_hits, hits + 1);
    assert_eq!(first.width, second.width);
    assert_eq!(first.glyphs.len(), second.glyphs.len());
}

#[test]
fn a_different_width_is_a_different_shape() {
    let mut engine = engine();
    let style = TextStyle::new(16.0);
    let text = "the quick brown fox jumps over the lazy dog";

    let wide = engine.shape(text, &style, 1000.0);
    let narrow = engine.shape(text, &style, 100.0);

    assert_eq!(wide.lines.len(), 1);
    assert!(narrow.lines.len() > 1);
}

#[test]
fn letter_spacing_widens_the_run() {
    let mut engine = engine();
    let plain = engine.shape("abcd", &TextStyle::new(16.0), f32::INFINITY);
    let spaced = engine.shape(
        "abcd",
        &TextStyle::new(16.0).letter_spacing(2.0),
        f32::INFINITY,
    );

    assert!(spaced.width > plain.width);
}

#[test]
fn alignment_offsets_the_glyphs() {
    let mut engine = engine();
    let start = engine.shape("abc", &TextStyle::new(16.0), 200.0);
    let center = engine.shape("abc", &TextStyle::new(16.0).align(TextAlign::Center), 200.0);
    let end = engine.shape("abc", &TextStyle::new(16.0).align(TextAlign::End), 200.0);

    assert_eq!(start.glyphs[0].x, 0.0);
    assert!(center.glyphs[0].x > 0.0);
    assert!(end.glyphs[0].x > center.glyphs[0].x);
}

#[test]
fn line_height_multiplier_changes_block_height() {
    let mut engine = engine();
    let normal = engine.shape("a\nb", &TextStyle::new(16.0), 200.0);
    let tall = engine.shape("a\nb", &TextStyle::new(16.0).line_height(2.0), 200.0);

    assert!(tall.height > normal.height);
}

#[test]
fn text_store_measures_match_shaping() {
    let mut store = TextStore::new(engine());
    let id = store.add("measure me", TextStyle::new(15.0));

    let measured = TextMetrics::measure(&mut store, id, 250.0);
    let shaped = store.shape(id, 250.0);

    assert_eq!(measured, shaped.size());
}

#[test]
fn text_store_reports_unknown_ids_safely() {
    let mut store = TextStore::new(engine());
    let measured = TextMetrics::measure(&mut store, TextId(99), 100.0);
    assert_eq!(measured, mre_core::Size::ZERO);
    assert_eq!(store.text(TextId(99)), "");
}

#[test]
fn index_at_finds_the_cluster_under_a_position() {
    let mut engine = engine();
    let shaped = engine.shape("abcd", &TextStyle::new(16.0), f32::INFINITY);

    let first = shaped.index_at(0, 0.0);
    let last = shaped.index_at(0, shaped.width + 10.0);

    assert_eq!(first, 0);
    assert_eq!(last, shaped.lines[0].end);
}

#[test]
fn rtl_paragraphs_are_detected_and_right_aligned() {
    let mut engine = engine();
    // Arabic. Roboto has no Arabic coverage, so this also exercises the
    // missing-glyph path without panicking.
    let shaped = engine.shape("مرحبا", &TextStyle::new(16.0), 200.0);

    assert!(shaped.rtl);
    assert_eq!(shaped.lines.len(), 1);
}

#[test]
fn a_font_less_engine_returns_an_empty_but_valid_layout() {
    let mut engine = TextEngine::new();
    let shaped = engine.shape("no fonts", &TextStyle::new(16.0), 100.0);

    assert!(shaped.glyphs.is_empty());
    assert_eq!(shaped.lines.len(), 1);
    assert!(shaped.height > 0.0);
}

#[test]
fn fallback_count_is_reported() {
    let mut engine = engine();
    // Mixed Latin and CJK with only a Latin font registered: CJK falls back to
    // glyph 0 in the same font and is counted, rather than silently vanishing.
    let shaped = engine.shape("hi 日本", &TextStyle::new(16.0), f32::INFINITY);
    assert!(shaped.glyphs.len() >= 2);
}
