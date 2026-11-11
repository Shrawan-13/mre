//! System font discovery.
//!
//! A window app starts from `TextStore::with_system_fonts()` — if that loads
//! no faces, every text run shapes to nothing and the app renders as bare
//! coloured rectangles. This is a regression test for exactly that: the
//! generic families used to be queried as `Family::Name("sans-serif")`, which
//! matches no typeface, because fontdb only resolves generics through its own
//! enum variants.
//!
//! Fails on a machine with no fonts installed at all, which is a broken
//! environment rather than an engine bug.

use mre_text::TextEngine;

#[test]
fn system_font_load_finds_faces() {
    let mut engine = TextEngine::new();
    let added = engine.load_system();
    assert!(
        added > 0,
        "load_system added {added} fonts on a machine that has fonts"
    );
}
