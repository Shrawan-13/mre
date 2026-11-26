//! Structural tests for the visual details that are easy to regress.
//!
//! These do not compare pixels; they assert the geometry that makes pixels
//! correct. A clipped tick mark or a cut border starts as a point that lands
//! too close to — or outside — its box.

use crate::ui::{icon_segments, is_circle};
use crate::view::IconKind;
use mre_core::Rect;

#[test]
fn icon_ink_stays_inside_the_box_at_every_size() {
    for size in [12.0, 14.0, 18.0, 24.0, 32.0] {
        let rect = Rect::new(10.0, 20.0, size, size);
        for kind in [
            IconKind::Check,
            IconKind::Close,
            IconKind::ChevronDown,
            IconKind::ChevronRight,
        ] {
            let (stroke, segments) = icon_segments(kind, rect);
            let half = stroke * 0.5;
            for (from, to) in segments {
                for point in [from, to] {
                    assert!(
                        point.x >= rect.x + half - 0.01
                            && point.y >= rect.y + half - 0.01
                            && point.x <= rect.right() - half + 0.01
                            && point.y <= rect.bottom() - half + 0.01,
                        "{kind:?} at {size}px put ink at {point:?} outside {rect:?} (half={half})"
                    );
                }
            }
        }
    }
}

#[test]
fn a_square_node_with_half_radius_uses_the_circle_primitive() {
    let knob = Rect::new(0.0, 3.0, 18.0, 18.0);
    assert!(is_circle(knob, 9.0), "slider knob must be a true circle");
    assert!(!is_circle(knob, 4.0), "a rounded square is not a circle");
    // A pill (wide, half-height radius) is not a circle either.
    assert!(!is_circle(Rect::new(0.0, 0.0, 36.0, 18.0), 9.0));
}

#[test]
fn a_check_is_not_squashed_by_a_rectangular_box() {
    // Icons use the smaller dimension and stay centred, so a wide box cannot
    // stretch a tick into a slash.
    let wide = Rect::new(0.0, 0.0, 40.0, 14.0);
    let square = Rect::new(0.0, 0.0, 14.0, 14.0);
    let (stroke_a, segments_a) = icon_segments(IconKind::Check, wide);
    let (stroke_b, segments_b) = icon_segments(IconKind::Check, square);
    assert_eq!(stroke_a, stroke_b);
    assert_eq!(segments_a, segments_b);
}
