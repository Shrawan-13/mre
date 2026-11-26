//! Regressions for the visual bugs reported from the Android build.
//!
//! Each test fails when a specific "looks wrong" defect returns: a clipped
//! border, a focus ring after a tap, or a slider that only responds to taps.

use mre_core::{Insets, Size};
use mre_runtime::{App, Modifiers};
use mre_text::{FontDesc, TextEngine, TextStore};

fn store() -> TextStore {
    let mut engine = TextEngine::new();
    engine
        .add_font_file(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/fonts/Roboto-Regular.ttf"
            ),
            FontDesc::new("Roboto"),
        )
        .expect("bundled font");
    TextStore::new(engine)
}

fn app(source: &str) -> App {
    let mut app = App::with_text_store(source, store()).expect("app");
    app.resize(Size::new(420.0, 640.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

const CARD: &str = r##"
mre.app {
  view = function()
    return mre.column {
      background = "#FFFFFF",
      mre.row {
        key = "card", width = 80, height = 48, radius = 8,
        background = "#FF0000",
        border = { color = "#0000FF", width = 2 },
      },
    }
  end,
}
"##;

const CHECK: &str = r##"
local checked = mre.signal(false)
mre.app {
  view = function()
    return mre.column {
      padding = 10,
      mre.checkbox {
        key = "box", label = "I agree", value = checked(),
        on_change = function(value) checked(value) end,
      },
    }
  end,
}
"##;

#[test]
fn damage_bands_are_padded_so_antialiasing_is_not_clipped() {
    let mut app = app(CARD);
    let rect = app.ui().rect_of_key("card").expect("card");
    let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    assert!(app.click(center.0, center.1).expect("click"));

    let regions = app.paint_region();
    assert!(
        regions.iter().any(|band| {
            band.x <= rect.x - 1.0
                && band.y <= rect.y - 1.0
                && band.x + band.w >= rect.right() + 1.0
                && band.y + band.h >= rect.bottom() + 1.0
        }),
        "no repaint band covered the antialiased edge of {rect:?}: {regions:?}"
    );
}

#[test]
fn pointer_focus_does_not_paint_a_focus_ring() {
    let mut app = app(CHECK);
    let rect = app.ui().rect_of_key("box").expect("checkbox");
    assert!(
        app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            .expect("click")
    );
    assert!(
        !app.ui().focus_visible(),
        "a tap must not show the focus ring"
    );

    assert!(app.key_with("tab", Modifiers::default()).expect("tab"));
    assert!(
        app.ui().focus_visible(),
        "keyboard navigation must show the ring"
    );
}

#[test]
fn a_touch_drag_moves_the_slider() {
    let source = include_str!("../../../examples/gallery.lua");
    let mut app = app(source);
    assert!(app.ui().text_content().contains("40%"));

    let rect = app.ui().rect_of_key("volume").expect("slider");
    let y = rect.y + rect.h / 2.0;
    app.touch_start(rect.x + 8.0, y);
    app.touch_move(rect.x + rect.w * 0.8, y)
        .expect("touch drag");
    app.touch_end(rect.x + rect.w * 0.8, y, 0.0, 0.0)
        .expect("touch release");

    let text = app.ui().text_content();
    assert!(
        text.contains("80%"),
        "slider did not follow the finger: {text}"
    );
}
