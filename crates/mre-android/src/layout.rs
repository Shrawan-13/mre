//! Density and safe-area conversion for Android hosts.
//!
//! Android's rule is: layout in dp, draw in pixels (`px = dp * dpi/160`).
//! MRE follows that exactly — `Viewport::from_physical` divides the window's
//! physical pixels by the density scale once. A host must therefore hand it
//! the *physical* size, not a size it has already divided. Doing both is the
//! classic "everything is tiny in the top-left corner" bug at 3x density.
//!
//! These helpers are pure and host-testable; the Android event loop just calls
//! them. See https://developer.android.com/training/multiscreen/screendensities
//! and https://developer.android.com/develop/ui/views/layout/insets.

use mre_core::{Insets, Size};

/// The size to pass to `App::resize`. Window managers report physical pixels;
/// the engine converts to logical (dp) itself.
#[must_use]
pub fn app_size(physical: Size, _scale: f32) -> Size {
    physical
}

/// Converts an Android content rect (physical pixels, edge coordinates) into
/// logical safe-area insets. The content rect excludes system bars and, while
/// the IME is up, the keyboard.
#[must_use]
pub fn content_insets(content: (i32, i32, i32, i32), physical: Size, scale: f32) -> Insets {
    let scale = if scale > 0.0 { scale } else { 1.0 };
    if content.2 <= content.0 || content.3 <= content.1 {
        return Insets::ZERO;
    }
    let left = (content.0.max(0) as f32 / scale).min(physical.w / scale);
    let top = (content.1.max(0) as f32 / scale).min(physical.h / scale);
    let right = ((physical.w - content.2.max(0) as f32) / scale).max(0.0);
    let bottom = ((physical.h - content.3.max(0) as f32) / scale).max(0.0);
    Insets::new(top, right, bottom, left)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mre_core::Viewport;

    #[test]
    fn physical_size_is_not_divided_twice() {
        let physical = Size::new(1080.0, 2400.0);
        let size = app_size(physical, 3.0);
        assert_eq!(size, physical);
        let viewport = Viewport::from_physical(size, 3.0);
        assert_eq!(viewport.size.w, 360.0);
        assert_eq!(viewport.size.h, 800.0);
    }

    #[test]
    fn safe_area_insets_are_logical() {
        // 1080x2400 pixels at 3x = 360x800 dp. Status bar 90px (30dp),
        // navigation bar 100px (33.3dp).
        let insets = content_insets((0, 90, 1080, 2300), Size::new(1080.0, 2400.0), 3.0);
        assert_eq!(insets.top, 30.0);
        assert!((insets.bottom - 100.0 / 3.0).abs() < 0.001);
        assert_eq!(insets.left, 0.0);
        assert_eq!(insets.right, 0.0);
    }

    #[test]
    fn an_empty_content_rect_means_no_insets() {
        assert_eq!(
            content_insets((0, 0, 0, 0), Size::new(1080.0, 2400.0), 3.0),
            Insets::ZERO
        );
    }
}
