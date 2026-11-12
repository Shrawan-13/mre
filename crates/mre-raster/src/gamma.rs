//! The weight of the text on the screen.
//!
//! A glyph's coverage is blended into the picture in encoded sRGB, but the
//! eye reads the result as light. A letter drawn in white over a black
//! background therefore looks lighter than the light it stands for, and the
//! stems of a small letter look thin and grey next to the solid ones.
//!
//! Skia corrects the coverage with a table chosen by the colour's
//! luminance, and a browser on Windows takes the same correction from the
//! platform. The table is Skia's: for each coverage it works out the
//! colour a blend in linear light would have shown and undoes the encoded
//! blend, so the text on the screen weighs what it should.

use std::sync::OnceLock;

use mre_core::Rgba8;

/// Chromium's contrast setting on Windows.
const CONTRAST: f32 = 1.0;

/// The tables, one for each luminance bucket.
static TABLES: OnceLock<[[u8; 256]; 8]> = OnceLock::new();

/// The luminance bucket of text drawn in `color`, as Skia picks a grayscale
/// mask's table: the Rec. 601 luma of the encoded colour, top three bits.
pub(crate) fn bucket(color: Rgba8) -> u8 {
    let luma =
        (u32::from(color.r()) * 54 + u32::from(color.g()) * 183 + u32::from(color.b()) * 19) >> 8;
    (luma >> 5) as u8
}

/// The coverage table for a luminance bucket.
pub(crate) fn table(bucket: u8) -> &'static [u8; 256] {
    let tables = TABLES.get_or_init(|| {
        let mut tables = [[0u8; 256]; 8];
        for (index, table) in tables.iter_mut().enumerate() {
            *table = build(index as u8);
        }
        tables
    });
    &tables[(bucket & 7) as usize]
}

/// The coverage table of a luminance bucket.
fn build(bucket: u8) -> [u8; 256] {
    let src = scale255(bucket) as f32 / 255.0;
    let dst = 1.0 - src;
    let (lin_src, lin_dst) = (to_linear(src), to_linear(dst));
    // The contrast tapers to nothing as the text approaches white.
    let contrast = CONTRAST * lin_dst;
    std::array::from_fn(|index| {
        let raw = index as f32 / 255.0;
        let a = raw + (1.0 - raw) * contrast * raw;
        let value = if (src - dst).abs() < 1.0 / 256.0 {
            a
        } else {
            // The colour a linear blend over the guessed background would
            // show, undone through the encoded blend the picture does.
            let out = from_linear(lin_src * a + (1.0 - a) * lin_dst);
            (out - dst) / (src - dst)
        };
        (255.0 * value + 0.5).floor().clamp(0.0, 255.0) as u8
    })
}

/// A three-bit bucket widened to a byte by repeating its bits.
fn scale255(bucket: u8) -> u32 {
    let base = u32::from(bucket & 7) << 5;
    base | base >> 3 | base >> 6
}

fn to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn from_linear(value: f32) -> f32 {
    if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_text_gets_a_heavier_coverage() {
        // Light text on a dark background is blended in encoded sRGB, so it
        // reads lighter than it is. The correction gives the faint pixels
        // more of their weight back.
        let table = table(bucket(Rgba8::WHITE));
        assert!(
            table[128] > 128,
            "half ink should read heavier: {}",
            table[128]
        );
        assert_eq!(table[0], 0);
        assert_eq!(table[255], 255);
    }

    #[test]
    fn dark_text_gets_a_lighter_coverage() {
        let table = table(bucket(Rgba8::BLACK));
        assert!(
            table[128] < 128,
            "half ink should read lighter: {}",
            table[128]
        );
        assert_eq!(table[0], 0);
        assert_eq!(table[255], 255);
    }
}
