//! Application-level tests: Lua in, geometry and pixels out.
//!
//! These are the tests that would catch a regression a user can feel: a button
//! that stops responding, a resize that keeps the old layout, a rotation that
//! keeps the old insets, an animation that never settles.

use mre_core::{Insets, Rect, Size};
use mre_layout::NodeId;
use mre_runtime::App;
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
    app.resize(Size::new(400.0, 300.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

const COUNTER: &str = r#"
local count = mre.signal(0)

mre.app {
  view = function()
    return mre.column {
      padding = 20,
      mre.text { "Count: " .. count(), size = 20 },
      mre.button {
        "Add",
        on_click = function() count(count() + 1) end,
      },
    }
  end,
}
"#;

fn button_centre(app: &App) -> (f32, f32) {
    let rect = app.ui().layout_result().rect(NodeId::from_raw(3));
    (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
}

#[test]
fn a_click_runs_the_handler_and_rerenders() {
    let mut app = app(COUNTER);
    assert!(app.runtime().tree().contains("Count: 0"));

    let (x, y) = button_centre(&app);
    assert!(app.click(x, y).expect("click"));

    assert!(
        app.runtime().tree().contains("Count: 1"),
        "tree was {}",
        app.runtime().tree()
    );

    assert!(app.click(x, y).expect("click"));
    assert!(app.runtime().tree().contains("Count: 2"));
}

#[test]
fn a_click_without_a_handler_does_nothing() {
    let mut app = app(COUNTER);
    let rebuilds = app.stats().rebuilds;

    assert!(!app.click(2.0, 2.0).expect("click"));
    assert!(!app.click(1000.0, 1000.0).expect("click"));
    assert_eq!(app.stats().rebuilds, rebuilds);
}

#[test]
fn an_idle_frame_does_no_work() {
    let mut app = app(COUNTER);
    let stats = app.stats();

    app.frame().expect("frame");
    app.frame().expect("frame");

    assert_eq!(app.stats().rebuilds, stats.rebuilds);
    assert_eq!(app.stats().layout_runs, stats.layout_runs);
    assert!(!app.needs_frame());
}

#[test]
fn a_disabled_click_target_is_not_hit() {
    let mut app = app(r##"
        mre.app {
          view = function()
            return mre.column {
              padding = 10,
              mre.column {
                width = 100, height = 40, background = "#FF0000",
                on_click = function() error("should not run") end,
              },
            }
          end,
        }
        "##);

    let rect = app.ui().layout_result().rect(NodeId::from_raw(2));
    assert_eq!(app.key_at(rect.x + 1.0, rect.y + 1.0), Some("@auto1"));
    assert!(app.click(-10.0, -10.0).is_ok());
}

#[test]
fn high_density_produces_a_physical_canvas_but_logical_layout() {
    let mut app = app(COUNTER);
    app.resize(Size::new(1600.0, 1200.0), 2.0, Insets::ZERO);

    assert_eq!(app.viewport().size, Size::new(800.0, 600.0));
    assert_eq!(app.viewport().scale, 2.0);
    assert_eq!(app.paint().width(), 1600);
    assert_eq!(app.paint().height(), 1200);
}

#[test]
fn rotation_swaps_the_viewport_and_the_insets() {
    let mut app = app(COUNTER);
    app.resize(
        Size::new(400.0, 800.0),
        1.0,
        Insets::new(50.0, 0.0, 20.0, 0.0),
    );
    assert_eq!(app.viewport().content(), Rect::new(0.0, 50.0, 400.0, 730.0));

    app.rotate();

    assert_eq!(app.viewport().size, Size::new(800.0, 400.0));
    assert_eq!(app.viewport().content(), Rect::new(20.0, 0.0, 730.0, 400.0));
    assert_eq!(app.paint().width(), 800);
    assert_eq!(app.paint().height(), 400);
}

#[test]
fn a_resize_keeps_the_content_inside_the_safe_area() {
    let mut app = app(r##"
        mre.app {
          view = function()
            return mre.column {
              background = "#FFFFFF",
              mre.row { height = 40, background = "#000000" },
            }
          end,
        }
        "##);
    app.resize(
        Size::new(400.0, 800.0),
        1.0,
        Insets::new(60.0, 0.0, 30.0, 0.0),
    );

    let row = app.ui().layout_result().rect(NodeId::from_raw(2));
    assert_eq!(row.y, 60.0);
    assert_eq!(row.w, 400.0);

    let canvas = app.paint();
    assert_eq!(
        canvas.pixel(200, 61),
        Some(mre_core::Rgba8::from_rgba(0, 0, 0, 255))
    );
    assert_eq!(
        canvas.pixel(200, 40),
        Some(mre_core::Rgba8::from_rgba(255, 255, 255, 255)),
        "the unsafe area must still be filled by the background"
    );
}

#[test]
fn changing_the_theme_repaints() {
    let mut app = app(r#"
        mre.app {
          view = function() return mre.column { } end,
        }
        "#);

    let before = app.paint().pixel(1, 1).expect("pixel");
    assert_eq!(before, mre_widgets::Theme::light().background);

    app.set_theme(mre_widgets::Theme::dark());
    let after = app.paint().pixel(1, 1).expect("pixel");
    assert_eq!(after, mre_widgets::Theme::dark().background);
}

#[test]
fn a_spring_animates_a_signal_until_it_settles() {
    let mut app = app(r#"
        local progress = mre.signal(0)
        local animate = mre.animate(progress)
        animate(100)

        mre.app {
          view = function()
            return mre.column {
              mre.text { string.format("%.0f", progress()), size = 20 },
            }
          end,
        }
        "#);

    assert!(app.runtime().animating());

    let mut elapsed = 0.0;
    while app.runtime().animating() && elapsed < 2.0 {
        app.tick(1.0 / 60.0).expect("tick");
        elapsed += 1.0 / 60.0;
    }

    assert!(!app.runtime().animating(), "spring did not settle");
    assert!(
        app.runtime().tree().contains("100"),
        "tree was {}",
        app.runtime().tree()
    );
    assert!(elapsed < 1.0, "animation took {elapsed}s");
}

#[test]
fn an_idle_app_does_not_ask_for_frames() {
    let app = app(COUNTER);
    assert!(!app.needs_frame());
}

#[test]
fn a_grid_dashboard_lays_out_without_panicking() {
    let mut app = app(r#"
        mre.app {
          view = function()
            local rows = {}
            for index = 1, 50 do
              rows[index] = mre.row {
                height = 24,
                mre.text { "row " .. index, width = 200 },
                mre.text { "123.45", width = 100, text_align = "end" },
              }
            end
            return mre.column { table.unpack(rows) }
          end,
        }
        "#);

    assert!(app.ui().node_count() >= 101);
    assert!(app.stats().nodes >= 101);
    let canvas = app.paint();
    assert_eq!(canvas.width(), 400);
}

fn table_source() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/table.lua"
    ))
    .expect("table example")
}

#[test]
fn a_virtualised_list_builds_only_visible_rows() {
    let mut app = App::with_text_store(&table_source(), store()).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();

    let nodes = app.ui().node_count();
    assert!(nodes < 250, "built {nodes} nodes for 50,000 rows");

    let text = app.ui().text_content();
    assert!(text.contains("Instrument 00001"), "text was {text}");
    assert!(
        !text.contains("Instrument 49000"),
        "rows outside the viewport must not be built"
    );
}

#[test]
fn scrolling_moves_the_window_without_building_the_data_set() {
    let mut app = App::with_text_store(&table_source(), store()).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();

    assert!(app.scroll(200.0, 300.0, 0.0, 20_000.0).expect("scroll"));

    let text = app.ui().text_content();
    assert!(!text.contains("Instrument 00001"), "window did not move");
    assert!(text.contains("Instrument 00715"), "text was {text}");
    assert!(text.contains("showing 715"), "text was {text}");
    assert!(app.ui().node_count() < 250);
}

#[test]
fn scrolling_is_clamped_by_the_handler() {
    let mut app = App::with_text_store(&table_source(), store()).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");

    app.scroll(200.0, 300.0, 0.0, -500.0).expect("scroll up");
    let text = app.ui().text_content();
    assert!(text.contains("Instrument 00001"), "text was {text}");
    assert!(text.contains("showing 1–"), "text was {text}");
}

#[test]
fn a_scroll_without_a_handler_is_ignored() {
    let mut app = app(COUNTER);
    assert!(!app.scroll(10.0, 10.0, 0.0, 10.0).expect("scroll"));
}
