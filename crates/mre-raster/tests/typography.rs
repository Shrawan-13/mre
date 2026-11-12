//! The letters of a word share one plane.
//!
//! The complaint these tests answer is visual: in a word, one letter looks
//! a pixel above another, and the stems are sometimes crisp and sometimes
//! two half-lit pixels. Both are decided when the outline is drawn, so they
//! are checked here on the pixels themselves.
//!
//! The hinter fits the heights of the letters to the pixel grid, so the
//! letters of a word start on the same row and end on the same row. The
//! coverage is the exact area of the outline inside each pixel, so a stem
//! that the design puts in the middle of a pixel is drawn as two pixels of
//! equal weight and not as a bright one and a dim one.

use mre_core::{Point, Rgba8};
use mre_raster::Canvas;
use mre_text::{TextEngine, TextStore, TextStyle};

fn font_path() -> std::path::PathBuf {
    std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/fonts/Roboto-Regular.ttf"
    ))
    .to_path_buf()
}

/// A store with one font in it.
fn store() -> TextStore {
    let mut engine = TextEngine::new();
    let data = std::fs::read(font_path()).expect("font");
    engine
        .add_font(data, 0, mre_text::FontDesc::new("Roboto"))
        .expect("font");
    TextStore::new(engine)
}

/// A word drawn on a white canvas, as the pixels it darkened.
fn draw(text: &str, size: f32, origin_y: f32) -> Canvas {
    let mut store = store();
    let id = store.add(text, TextStyle::new(size));
    let shaped = store.shape(id, 1000.0);

    let mut canvas = Canvas::new(120, 40);
    canvas.clear(Rgba8::WHITE);
    canvas.draw_text(
        store.engine(),
        &shaped,
        size,
        Point::new(4.0, origin_y),
        Rgba8::BLACK,
    );
    canvas
}

/// The ink of a word, grouped into letters: for each letter, the first and
/// the last row it reaches.
fn letters(canvas: &Canvas) -> Vec<(u32, u32)> {
    let mut groups: Vec<(u32, u32)> = Vec::new();
    let mut current: Option<(u32, u32)> = None;
    for x in 0..canvas.width() {
        let mut top = u32::MAX;
        let mut bottom = 0u32;
        for y in 0..canvas.height() {
            let Some(pixel) = canvas.pixel(x, y) else {
                continue;
            };
            if pixel.r() < 200 {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }
        if top == u32::MAX {
            // A column with no ink ends the letter.
            if let Some(group) = current.take() {
                groups.push(group);
            }
            continue;
        }
        current = Some(match current {
            Some((group_top, group_bottom)) => (group_top.min(top), group_bottom.max(bottom)),
            None => (top, bottom),
        });
    }
    if let Some(group) = current {
        groups.push(group);
    }
    groups
}

#[test]
fn the_letters_of_a_word_share_their_rows() {
    // The size matters: the complaint shows up where the design's heights
    // do not land on the grid by themselves.
    for size in [11.0f32, 13.0, 14.0, 17.0] {
        let canvas = draw("one", size, 4.0);
        let groups = letters(&canvas);
        assert!(
            groups.len() >= 2,
            "the word should have ink at {size}px: {groups:?}"
        );

        // The letters of a word are fitted to the grid together, so they
        // start on the same row and end on the same row. A letter that ends
        // a row above the others is the "one letter feels above another"
        // complaint.
        let top = groups.iter().map(|(top, _)| *top).min().expect("ink");
        let bottom = groups.iter().map(|(_, bottom)| *bottom).max().expect("ink");
        for (index, (letter_top, letter_bottom)) in groups.iter().enumerate() {
            assert_eq!(
                *letter_top, top,
                "letter {index} starts a row above the others at {size}px: {groups:?}"
            );
            assert_eq!(
                *letter_bottom, bottom,
                "letter {index} ends a row below the others at {size}px: {groups:?}"
            );
        }
    }
}

#[test]
fn the_baseline_lands_on_a_whole_pixel() {
    // The hinter fitted the letters to the grid, and the grid is only the
    // grid if the baseline is on it. A text that starts a third of a pixel
    // lower is drawn on the same rows as the text that starts on the
    // pixel, because the baseline is put on the pixel when it is drawn.
    let whole = draw("one", 14.0, 4.0);
    let nudged = draw("one", 14.0, 4.3);
    assert_eq!(whole.pixels(), nudged.pixels());
}
