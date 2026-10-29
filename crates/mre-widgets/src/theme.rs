//! The theme: colours, radii and spacing in one place.
//!
//! Widgets never hard-code a colour. Everything visual resolves through the
//! theme, so dark mode, high contrast and brand palettes are a data change.

use mre_core::Rgba8;

/// A complete visual palette.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// Window background.
    pub background: Rgba8,
    /// Cards, panels, table rows.
    pub surface: Rgba8,
    /// Alternating row colour.
    pub surface_alt: Rgba8,
    /// Hairlines and outlines.
    pub border: Rgba8,
    /// Primary text.
    pub text: Rgba8,
    /// Secondary text.
    pub text_muted: Rgba8,
    /// Accent, for buttons and selection.
    pub accent: Rgba8,
    /// Text on top of the accent.
    pub on_accent: Rgba8,
    /// Corner radius for cards and buttons.
    pub radius: f32,
    /// Base spacing unit.
    pub spacing: f32,
    /// Default body font size.
    pub font_size: f32,
    /// Default button height.
    pub control_height: f32,
}

impl Theme {
    /// A clean light palette.
    #[must_use]
    pub fn light() -> Self {
        Self {
            background: hex("#FFFFFF"),
            surface: hex("#F7F8FA"),
            surface_alt: hex("#EEF1F5"),
            border: hex("#D9DEE7"),
            text: hex("#111827"),
            text_muted: hex("#6B7280"),
            accent: hex("#2563EB"),
            on_accent: hex("#FFFFFF"),
            radius: 8.0,
            spacing: 8.0,
            font_size: 14.0,
            control_height: 32.0,
        }
    }

    /// A low-glare dark palette.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            background: hex("#0B0E14"),
            surface: hex("#151A23"),
            surface_alt: hex("#1C2330"),
            border: hex("#2A3342"),
            text: hex("#E5E9F0"),
            text_muted: hex("#98A2B3"),
            accent: hex("#5B8DEF"),
            on_accent: hex("#0B0E14"),
            radius: 8.0,
            spacing: 8.0,
            font_size: 14.0,
            control_height: 32.0,
        }
    }

    /// High-contrast light palette for accessibility.
    #[must_use]
    pub fn high_contrast() -> Self {
        Self {
            background: hex("#FFFFFF"),
            surface: hex("#FFFFFF"),
            surface_alt: hex("#F0F0F0"),
            border: hex("#000000"),
            text: hex("#000000"),
            text_muted: hex("#333333"),
            accent: hex("#0033CC"),
            on_accent: hex("#FFFFFF"),
            radius: 4.0,
            spacing: 8.0,
            font_size: 14.0,
            control_height: 36.0,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

fn hex(value: &str) -> Rgba8 {
    Rgba8::parse_hex(value).expect("theme colours are valid hex literals")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_are_opaque() {
        for theme in [Theme::light(), Theme::dark(), Theme::high_contrast()] {
            assert_eq!(theme.background.a(), 255);
            assert_eq!(theme.text.a(), 255);
            assert_eq!(theme.accent.a(), 255);
        }
    }

    #[test]
    fn dark_text_is_light_and_light_text_is_dark() {
        assert!(Theme::light().text.r() < 64);
        assert!(Theme::dark().text.r() > 192);
    }
}
