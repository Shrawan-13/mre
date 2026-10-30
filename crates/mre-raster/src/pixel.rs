//! Premultiplied pixel blending.
//!
//! tiny-skia is fast for shapes but slow for the two operations a UI does most:
//! filling axis-aligned rectangles and drawing glyphs. Both are simple enough to
//! do directly, which turns a table row from a masked path fill into a memory
//! write and a glyph from a path conversion into an alpha blit.

use mre_core::Rgba8;

/// `value / 255`, rounded.
#[inline]
fn div255(value: u32) -> u32 {
    (value + 127) / 255
}

/// Reads a premultiplied pixel from a canvas.
#[inline]
fn read(data: &[u8], index: usize) -> [u8; 4] {
    [
        data[index],
        data[index + 1],
        data[index + 2],
        data[index + 3],
    ]
}

/// Fills an axis-aligned pixel rectangle, clipped to `clip`.
///
/// `clip` is `(x0, y0, x1, y1)` with exclusive maxima, in physical pixels.
pub(crate) fn fill_rect(
    data: &mut [u8],
    width: usize,
    clip: (i32, i32, i32, i32),
    rect: (i32, i32, i32, i32),
    color: Rgba8,
) {
    let alpha = u32::from(color.a());
    if alpha == 0 {
        return;
    }
    let x0 = rect.0.max(clip.0).max(0);
    let y0 = rect.1.max(clip.1).max(0);
    let x1 = rect.2.min(clip.2).min(width as i32);
    let y1 = rect.3.min(clip.3);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let x0 = x0 as usize;
    let x1 = x1 as usize;

    if alpha == 255 {
        let pixel = [color.r(), color.g(), color.b(), 255];
        for y in y0..y1 {
            let start = y as usize * width * 4 + x0 * 4;
            let end = y as usize * width * 4 + x1 * 4;
            for slot in data[start..end].chunks_exact_mut(4) {
                slot.copy_from_slice(&pixel);
            }
        }
        return;
    }

    let source = [
        div255(u32::from(color.r()) * alpha) as u8,
        div255(u32::from(color.g()) * alpha) as u8,
        div255(u32::from(color.b()) * alpha) as u8,
        alpha as u8,
    ];
    let inverse = 255 - alpha;
    for y in y0..y1 {
        let row = y as usize * width * 4;
        for x in x0..x1 {
            let index = row + x * 4;
            let under = read(data, index);
            for channel in 0..3 {
                data[index + channel] = (u32::from(source[channel])
                    + div255(u32::from(under[channel]) * inverse))
                    as u8;
            }
            data[index + 3] = (alpha + div255(u32::from(under[3]) * inverse)) as u8;
        }
    }
}

/// Blends one alpha-covered glyph bitmap into the canvas.
///
/// `alpha` is row-major, `width * height` bytes, 0 = transparent. `at` is the
/// top-left pixel of the bitmap. The clip is applied per row and per span, so a
/// glyph cut by a scroll viewport costs only the pixels it actually touches.
pub(crate) fn blit_alpha(
    data: &mut [u8],
    stride: usize,
    clip: (i32, i32, i32, i32),
    at: (i32, i32),
    size: (u32, u32),
    alpha: &[u8],
    color: Rgba8,
) {
    let color_alpha = u32::from(color.a());
    if color_alpha == 0 {
        return;
    }
    let (bitmap_width, bitmap_height) = size;
    let left = at.0.max(clip.0);
    let right = (at.0 + bitmap_width as i32).min(clip.2);
    let top = at.1.max(clip.1);
    let bottom = (at.1 + bitmap_height as i32).min(clip.3);
    if right <= left || bottom <= top {
        return;
    }

    let red = u32::from(color.r());
    let green = u32::from(color.g());
    let blue = u32::from(color.b());

    for y in top..bottom {
        let source_row = (y - at.1) as usize * bitmap_width as usize;
        let destination_row = y as usize * stride * 4;
        for x in left..right {
            let coverage = u32::from(alpha[source_row + (x - at.0) as usize]);
            if coverage == 0 {
                continue;
            }
            let effective = div255(coverage * color_alpha);
            let inverse = 255 - effective;
            let index = destination_row + x as usize * 4;
            let under = read(data, index);
            data[index] = (div255(red * effective) + div255(u32::from(under[0]) * inverse)) as u8;
            data[index + 1] =
                (div255(green * effective) + div255(u32::from(under[1]) * inverse)) as u8;
            data[index + 2] =
                (div255(blue * effective) + div255(u32::from(under[2]) * inverse)) as u8;
            data[index + 3] = (effective + div255(u32::from(under[3]) * inverse)) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opaque_fill_ignores_what_was_under_it() {
        let mut data = vec![10_u8; 3 * 3 * 4];
        fill_rect(
            &mut data,
            3,
            (0, 0, 3, 3),
            (1, 0, 3, 1),
            Rgba8::from_rgba(255, 0, 0, 255),
        );
        assert_eq!(&data[4..8], &[255, 0, 0, 255]);
        assert_eq!(&data[8..12], &[255, 0, 0, 255]);
        assert_eq!(&data[0..4], &[10, 10, 10, 10]);
    }

    #[test]
    fn a_half_transparent_fill_blends() {
        let data = [0_u8, 0, 0, 255];
        let mut copy = data.to_vec();
        fill_rect(
            &mut copy,
            1,
            (0, 0, 1, 1),
            (0, 0, 1, 1),
            Rgba8::from_rgba(255, 255, 255, 128),
        );
        assert_eq!(copy[0], 128);
        assert_eq!(copy[3], 255);
    }

    #[test]
    fn a_glyph_respects_the_clip_span() {
        let mut data = vec![0_u8; 3 * 4];
        let alpha = [255_u8, 255, 255];
        blit_alpha(
            &mut data,
            3,
            (1, 0, 2, 1),
            (0, 0),
            (3, 1),
            &alpha,
            Rgba8::WHITE,
        );
        assert_eq!(&data[0..4], &[0, 0, 0, 0]);
        assert_eq!(&data[4..8], &[255, 255, 255, 255]);
        assert_eq!(&data[8..12], &[0, 0, 0, 0]);
    }
}
