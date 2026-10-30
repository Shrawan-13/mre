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

#[test]
fn dragging_off_a_button_does_not_click_it() {
    let mut app = app(COUNTER);
    let (x, y) = button_centre(&app);

    app.pointer_move(x, y);
    app.pointer_down(x, y);
    app.pointer_move(2.0, 2.0);

    assert!(!app.pointer_up(2.0, 2.0).expect("release"));
    assert!(app.runtime().tree().contains("Count: 0"));
}

#[test]
fn hovering_repaints_without_rebuilding_the_view() {
    let mut app = app(COUNTER);
    let rebuilds = app.stats().rebuilds;
    let (x, y) = button_centre(&app);
    let before = app.paint().pixel(x as u32, y as u32).expect("pixel");

    assert!(app.pointer_move(x, y));
    let after = app.paint().pixel(x as u32, y as u32).expect("pixel");

    assert_ne!(before, after);
    assert_eq!(app.stats().rebuilds, rebuilds, "hover must not re-render");
}

fn gallery() -> App {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/gallery.lua"
    ))
    .expect("gallery example");
    let mut app = App::with_text_store(&source, store()).expect("app");
    app.resize(Size::new(420.0, 500.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

#[test]
fn a_checkbox_toggles_on_click() {
    let mut app = gallery();
    let row = app.ui().rect_of_key("show-grid").expect("checkbox");
    let box_x = (row.x + 9.0) as u32;
    let box_y = (row.y + 12.0) as u32;

    let before = app.paint().pixel(box_x, box_y).expect("pixel");
    assert!(app.click(row.x + 9.0, row.y + 12.0).expect("click"));
    let after = app.paint().pixel(box_x, box_y).expect("pixel");

    assert_ne!(before, after, "the box should change when checked");
}

#[test]
fn a_slider_follows_a_drag() {
    let mut app = gallery();
    assert!(app.ui().text_content().contains("40%"));

    let rect = app.ui().rect_of_key("volume").expect("slider");
    let y = rect.y + rect.h / 2.0;

    app.pointer_down(rect.x + 8.0, y);
    app.pointer_drag(rect.x + rect.w * 0.8, y).expect("drag");
    app.pointer_up(rect.x + rect.w * 0.8, y).expect("release");

    let text = app.ui().text_content();
    assert!(text.contains("80%"), "text was {text}");
}

#[test]
fn clicking_a_slider_track_jumps_to_the_position() {
    let mut app = gallery();
    let rect = app.ui().rect_of_key("quality").expect("slider");
    let y = rect.y + rect.h / 2.0;

    // A quarter of the way along a 0..4 range is 1.
    app.click(rect.x + rect.w * 0.25, y).expect("click");

    let text = app.ui().text_content();
    assert!(text.contains("\n1\n"), "text was {text}");
}

#[test]
fn a_toggle_flips_on_click() {
    let mut app = gallery();
    let rect = app.ui().rect_of_key("toggle").expect("toggle");
    let center = (rect.x + rect.w - 6.0, rect.y + rect.h / 2.0);
    let before = app.paint().pixel(center.0 as u32, center.1 as u32);

    assert!(app.click(center.0, center.1).expect("click"));
    let after = app.paint().pixel(center.0 as u32, center.1 as u32);
    assert_ne!(before, after);
}

fn form() -> App {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/form.lua"
    ))
    .expect("form example");
    let mut app = App::with_text_store(&source, store()).expect("app");
    app.resize(Size::new(420.0, 360.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

fn click_field(app: &mut App, key: &str) {
    let rect = app.ui().rect_of_key(key).expect("field");
    app.click(rect.x + 20.0, rect.y + rect.h / 2.0)
        .expect("click");
}

#[test]
fn clicking_a_field_focuses_it() {
    let mut app = form();
    assert_eq!(app.focused(), None);

    click_field(&mut app, "name");
    assert_eq!(app.focused(), Some("name"));

    click_field(&mut app, "email");
    assert_eq!(app.focused(), Some("email"));
}

#[test]
fn typing_updates_the_focused_field() {
    let mut app = form();
    click_field(&mut app, "name");

    for character in ["A", "d", "a"] {
        assert!(app.insert_text(character).expect("insert"));
    }

    assert_eq!(app.ui().input_text("name"), Some("Ada"));
    assert!(app.ui().text_content().contains("Ada"));
}

#[test]
fn backspace_and_arrows_edit_at_the_caret() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("ab").expect("insert");

    assert!(app.key("left", false).expect("left"));
    app.insert_text("X").expect("insert");
    assert_eq!(app.ui().input_text("name"), Some("aXb"));

    assert!(app.key("backspace", false).expect("backspace"));
    assert_eq!(app.ui().input_text("name"), Some("ab"));

    assert!(app.key("home", false).expect("home"));
    assert!(app.key("delete", false).expect("delete"));
    assert_eq!(app.ui().input_text("name"), Some("b"));
}

#[test]
fn tab_moves_focus_between_fields() {
    let mut app = form();
    click_field(&mut app, "name");

    assert!(app.key("tab", false).expect("tab"));
    assert_eq!(app.focused(), Some("email"));
    assert!(app.key("tab", false).expect("tab"));
    assert_eq!(app.focused(), Some("password"));
    assert!(app.key("tab", false).expect("tab"));
    assert_eq!(app.focused(), Some("name"));
    assert!(app.key("tab", true).expect("shift-tab"));
    assert_eq!(app.focused(), Some("password"));
}

#[test]
fn enter_submits_the_field() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("Ada").expect("insert");
    assert!(app.key("enter", false).expect("enter"));

    let text = app.ui().text_content();
    assert!(text.contains("Submitted Ada"), "text was {text}");
}

#[test]
fn escape_blurs_the_field() {
    let mut app = form();
    click_field(&mut app, "name");
    assert_eq!(app.focused(), Some("name"));

    assert!(app.key("escape", false).expect("escape"));
    assert_eq!(app.focused(), None);
}

#[test]
fn a_password_field_renders_bullets_not_the_value() {
    let mut app = form();
    click_field(&mut app, "password");
    app.insert_text("secret").expect("insert");

    assert_eq!(app.ui().input_text("password"), Some("secret"));

    let rect = app.ui().rect_of_key("password").expect("field");
    let canvas = app.paint();
    let mut dark = 0;
    for y in (rect.y as u32)..((rect.y + rect.h) as u32) {
        for x in (rect.x as u32)..((rect.x + rect.w) as u32) {
            if let Some(pixel) = canvas.pixel(x, y) {
                if pixel.r() < 120 {
                    dark += 1;
                }
            }
        }
    }
    assert!(dark > 20, "the password field should not be empty");
}

#[test]
fn a_click_outside_a_field_blurs_it() {
    let mut app = form();
    click_field(&mut app, "name");
    assert_eq!(app.focused(), Some("name"));

    app.click(400.0, 340.0).expect("click");
    assert_eq!(app.focused(), None);
}

#[test]
fn the_scrollbar_thumb_tracks_the_offset() {
    let mut app = App::with_text_store(&table_source(), store()).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();

    let x = 891;
    let y = 388;
    let before = app.paint().pixel(x, y).expect("pixel");
    assert_eq!(
        before,
        mre_core::Rgba8::from_rgba(229, 231, 235, 255),
        "the thumb starts at the top, so the middle of the track is empty"
    );

    app.scroll(200.0, 300.0, 0.0, 700_000.0).expect("scroll");
    let after = app.paint().pixel(x, y).expect("pixel");
    assert_eq!(
        after,
        mre_core::Rgba8::from_rgba(156, 163, 175, 255),
        "the thumb should be halfway down"
    );
}

#[test]
fn a_bound_animation_repaints_without_rebuilding() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/bindings.lua"
    ))
    .expect("bindings example");
    let mut app = App::with_text_store(&source, store()).expect("app");
    app.resize(Size::new(420.0, 320.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();

    let rebuilds = app.stats().rebuilds;
    let card = app.ui().rect_of_key("card").expect("card");
    let probe = ((card.x + 10.0) as u32, (card.y + 10.0) as u32);
    let before = app.paint().pixel(probe.0, probe.1).expect("pixel");

    let mut elapsed = 0.0;
    while app.runtime().animating() && elapsed < 2.0 {
        app.tick(1.0 / 60.0).expect("tick");
        elapsed += 1.0 / 60.0;
    }

    assert_eq!(
        app.stats().rebuilds,
        rebuilds,
        "a bound animation must not rebuild the view"
    );
    assert!(elapsed < 1.0, "animation took {elapsed}s");

    let after = app.paint().pixel(probe.0, probe.1).expect("pixel");
    assert_ne!(before, after, "the card should have moved");

    let animated = app.ui().animated("card").expect("animated");
    assert_eq!(animated.opacity, Some(1.0));
    assert!(animated.dy.abs() < 0.5, "slide should settle: {animated:?}");

    // The card's layout rect is unchanged; only the painted position moved.
    assert_eq!(app.ui().rect_of_key("card"), Some(card));
}
