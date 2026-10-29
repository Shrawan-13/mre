//! Colour: packed sRGB8, converted to linear only at upload time.

/// An 8-bit sRGB colour with alpha, packed as `0xRRGGBBAA`.
///
/// The engine stores colour as sRGB8 and **defers linearisation to the GPU
/// upload / shader**. Premultiplying and linearising per-draw-call on the CPU
/// was one of the classic mistakes of early 2D engines: it turns a `u32` compare
/// into a float compare, which destroys batching.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rgba8(pub u32);

impl Rgba8 {
    /// Fully transparent black.
    pub const TRANSPARENT: Self = Self(0x0000_0000);
    /// Opaque black.
    pub const BLACK: Self = Self(0x0000_00FF);
    /// Opaque white.
    pub const WHITE: Self = Self(0xFFFF_FFFF);

    /// Builds a colour from its channels.
    #[inline]
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32))
    }

    /// Parses `#RRGGBB` or `#RRGGBBAA`. Returns `None` on malformed input.
    ///
    /// Only used on the cold path (theme loading, tests); never per frame.
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        let hex = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        match s.len() {
            6 => Some(Self::from_rgba(hex(0)?, hex(2)?, hex(4)?, 0xFF)),
            8 => Some(Self::from_rgba(hex(0)?, hex(2)?, hex(4)?, hex(6)?)),
            _ => None,
        }
    }

    /// Red channel, sRGB-encoded.
    #[inline]
    pub const fn r(self) -> u8 {
        (self.0 >> 24) as u8
    }

    /// Green channel, sRGB-encoded.
    #[inline]
    pub const fn g(self) -> u8 {
        (self.0 >> 16) as u8
    }

    /// Blue channel, sRGB-encoded.
    #[inline]
    pub const fn b(self) -> u8 {
        (self.0 >> 8) as u8
    }

    /// Alpha channel. Opaque is `255`.
    #[inline]
    pub const fn a(self) -> u8 {
        self.0 as u8
    }

    /// `[r, g, b, a]` as linear-light floats, for GPU upload.
    ///
    /// This is the only place sRGB decoding happens on the CPU. Prefer doing it
    /// in the shader with an `Srgb` texture format when possible.
    pub fn to_linear(self) -> [f32; 4] {
        let f = srgb_to_linear;
        [
            f(self.r()),
            f(self.g()),
            f(self.b()),
            self.a() as f32 / 255.0,
        ]
    }
}

/// sRGB transfer function, inverse (encoded -> linear).
#[inline]
fn srgb_to_linear(c: u8) -> f32 {
    let s = c as f32 / 255.0;
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_channels() {
        let c = Rgba8::from_rgba(0x12, 0x34, 0x56, 0x78);
        assert_eq!((c.r(), c.g(), c.b(), c.a()), (0x12, 0x34, 0x56, 0x78));
        assert_eq!(c.0, 0x1234_5678);
    }

    #[test]
    fn parses_hex_with_and_without_alpha() {
        assert_eq!(
            Rgba8::parse_hex("#ff0080"),
            Some(Rgba8::from_rgba(255, 0, 128, 255))
        );
        assert_eq!(
            Rgba8::parse_hex("#ff008080"),
            Some(Rgba8::from_rgba(255, 0, 128, 128))
        );
        assert_eq!(Rgba8::parse_hex("ff0080"), None);
        assert_eq!(Rgba8::parse_hex("#fff"), None);
    }

    #[test]
    fn linear_white_is_one() {
        let lin = Rgba8::WHITE.to_linear();
        assert!((lin[0] - 1.0).abs() < 1e-6);
        assert!((lin[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn linear_black_is_zero() {
        let lin = Rgba8::BLACK.to_linear();
        assert_eq!(lin[0], 0.0);
        assert!((lin[3] - 1.0).abs() < 1e-6, "alpha is not gamma-encoded");
    }

    #[test]
    fn color_is_four_bytes() {
        assert_eq!(core::mem::size_of::<Rgba8>(), 4);
    }
}
