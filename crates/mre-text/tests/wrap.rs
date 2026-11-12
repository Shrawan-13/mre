//! Line breaking.
//!
//! The layout measures a text, then re-measures it at the width it has just
//! measured. Those two widths can differ by a float ULP, and a strict
//! comparison in the line breaker then breaks a word that fits — a button
//! label wraps onto a second line for no visible reason.

use mre_text::{TextEngine, TextStyle};

/// One unit in the last place of the float.
fn one_ulp_below(value: f32) -> f32 {
    f32::from_bits(value.to_bits() - 1)
}

#[test]
fn a_word_that_fits_is_not_broken_by_a_float_ulp() {
    let mut engine = TextEngine::new();
    engine.load_system();
    let style = TextStyle {
        size: 14.0,
        weight: 500,
        ..TextStyle::default()
    };

    for word in ["Done", "Active", "Ship the demo", "Push to GitHub"] {
        let wide = engine.shape(word, &style, 1.0e6);
        let width = wide.size().w;
        assert_eq!(wide.lines.len(), 1, "{word:?} should fit on one line");

        let narrow = engine.shape(word, &style, one_ulp_below(width));
        assert_eq!(
            narrow.lines.len(),
            1,
            "{word:?} must not be broken at its own width (measured {width})"
        );
    }
}

#[test]
fn a_word_that_does_not_fit_is_broken() {
    let mut engine = TextEngine::new();
    engine.load_system();
    let style = TextStyle {
        size: 14.0,
        weight: 500,
        ..TextStyle::default()
    };

    let wide = engine.shape("Push to GitHub", &style, 1.0e6);
    let narrow = engine.shape("Push to GitHub", &style, wide.size().w / 2.0);
    assert!(
        narrow.lines.len() > 1,
        "a word that does not fit should be broken"
    );
}

#[test]
fn the_box_holds_the_ink() {
    // The letters are optically centred, which moves them within their box.
    // The box grows to hold them, so the ink never leaves it — a descender
    // that pokes out would collide with the text below.
    let mut engine = TextEngine::new();
    engine.load_system();
    for (size, weight) in [(48.0_f32, 600_u16), (16.0, 600), (13.0, 400)] {
        let style = TextStyle {
            size,
            weight,
            ..TextStyle::default()
        };
        let text = "Nothing on your plate";
        let shaped = engine.shape(text, &style, 1.0e6);
        let baseline = shaped.lines.first().map_or(0.0, |line| line.baseline);
        let (top, bottom) = engine.ink(text, &style);
        assert!(
            baseline - top >= -0.01,
            "the ink of {size}px text pokes above its box"
        );
        assert!(
            baseline + bottom <= shaped.height + 0.01,
            "the ink of {size}px text pokes below its box"
        );
    }
}
