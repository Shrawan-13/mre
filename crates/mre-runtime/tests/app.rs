//! Application-level tests: Lua in, geometry and pixels out.
//!
//! These are the tests that would catch a regression a user can feel: a button
//! that stops responding, a resize that keeps the old layout, a rotation that
//! keeps the old insets, an animation that never settles.

use mre_core::{Insets, Rect, Rgba8, Size};
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
    // The label sits in the middle of the button, so the probe is taken from
    // the padding beside it, where the background is.
    let probe = (x - 8.0) as u32;
    let before = app.paint().pixel(probe, y as u32).expect("pixel");

    assert!(app.pointer_move(x, y));
    let after = app.paint().pixel(probe, y as u32).expect("pixel");

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
    assert_eq!(
        app.focused(),
        Some("@auto1"),
        "the Create button is focusable too"
    );
    assert!(app.key("tab", false).expect("tab"));
    assert_eq!(app.focused(), Some("name"));
    assert!(app.key("tab", true).expect("shift-tab"));
    assert_eq!(app.focused(), Some("@auto1"));
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

#[test]
fn the_accessibility_tree_names_the_controls() {
    use mre_widgets::Role;

    let app = gallery();
    let semantics = app.semantics();
    let checkbox = semantics
        .iter()
        .find(|s| s.role == Role::CheckBox)
        .expect("checkbox");
    assert_eq!(checkbox.label, "Show grid");
    assert!(checkbox.focusable);

    let toggle = semantics
        .iter()
        .find(|s| s.role == Role::Switch)
        .expect("switch");
    assert!(!toggle.label.is_empty());

    let mut app = form();
    let semantics = app.semantics();
    let fields: Vec<_> = semantics
        .iter()
        .filter(|s| s.role == Role::TextField)
        .collect();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].label, "Full name");
    assert!(fields[0].focusable);
    assert!(!fields[0].focused);

    click_field(&mut app, "name");
    let focused = app
        .semantics()
        .into_iter()
        .find(|node| node.focused)
        .expect("a focused node");
    assert_eq!(focused.role, Role::TextField);
    assert_eq!(focused.key.as_deref(), Some("name"));
}

#[test]
fn tab_focuses_a_button_and_space_activates_it() {
    let mut app = app(COUNTER);
    assert!(app.runtime().tree().contains("Count: 0"));

    assert!(app.key("tab", false).expect("tab"));
    assert!(app.focused().is_some());

    assert!(app.key("space", false).expect("activate"));
    assert!(
        app.runtime().tree().contains("Count: 1"),
        "tree was {}",
        app.runtime().tree()
    );
}

#[test]
fn enter_also_activates_a_focused_button() {
    let mut app = app(COUNTER);
    app.key("tab", false).expect("tab");
    assert!(app.key("enter", false).expect("activate"));
    assert!(app.runtime().tree().contains("Count: 1"));
}

#[test]
fn clicking_inside_a_field_places_the_caret() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("abcd").expect("type");

    let rect = app.ui().rect_of_key("name").expect("field");
    app.click(rect.x + 10.0, rect.y + rect.h / 2.0)
        .expect("click near the start");
    app.insert_text("X").expect("type");
    assert_eq!(app.ui().input_text("name"), Some("Xabcd"));

    app.click(rect.x + rect.w - 12.0, rect.y + rect.h / 2.0)
        .expect("click near the end");
    app.insert_text("Y").expect("type");
    assert_eq!(app.ui().input_text("name"), Some("XabcdY"));
}

#[test]
fn shift_arrows_select_and_typing_replaces() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("hello").expect("type");
    app.key("home", false).expect("home");

    assert!(app.key("right", true).expect("shift-right"));
    assert!(app.key("right", true).expect("shift-right"));
    assert_eq!(app.copied_text().as_deref(), Some("he"));

    app.insert_text("YE").expect("replace");
    assert_eq!(app.ui().input_text("name"), Some("YEllo"));
    assert_eq!(app.copied_text(), None);
}

#[test]
fn dragging_selects_and_cut_returns_the_selection() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("abcdef").expect("type");

    let rect = app.ui().rect_of_key("name").expect("field");
    let y = rect.y + rect.h / 2.0;
    app.pointer_down(rect.x + 9.0, y);
    app.pointer_drag(rect.x + rect.w, y).expect("drag");
    app.pointer_up(rect.x + rect.w, y).expect("release");

    assert_eq!(app.copied_text().as_deref(), Some("abcdef"));
    let cut = app.cut().expect("cut").expect("a selection");
    assert_eq!(cut, "abcdef");
    assert_eq!(app.ui().input_text("name"), Some(""));
}

#[test]
fn paste_replaces_a_selection_and_select_all_clears_it() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("old").expect("type");

    assert!(app.select_all());
    assert_eq!(app.copied_text().as_deref(), Some("old"));

    app.paste("new name").expect("paste");
    assert_eq!(app.ui().input_text("name"), Some("new name"));

    app.select_all();
    app.key("backspace", false).expect("delete selection");
    assert_eq!(app.ui().input_text("name"), Some(""));
}

#[test]
fn on_change_fires_on_every_edit() {
    let mut app = app(r#"
        local seen = mre.signal("")

        mre.app {
          view = function()
            return mre.column {
              padding = 10,
              mre.text { "Echo: " .. seen() },
              mre.textfield {
                key = "field",
                value = seen(),
                on_change = function(text) seen(text) end,
              },
            }
          end,
        }
        "#);

    let rect = app.ui().rect_of_key("field").expect("field");
    app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
        .expect("focus");

    app.insert_text("ab").expect("type");
    app.insert_text("cd").expect("type");

    assert_eq!(app.ui().input_text("field"), Some("abcd"));
    assert!(
        app.runtime().tree().contains("Echo: abcd"),
        "on_change should publish every edit, tree was {}",
        app.runtime().tree()
    );
}

#[test]
fn an_ime_can_replace_the_full_editor_state() {
    let mut app = app(r#"
        local seen = mre.signal("")

        mre.app {
          view = function()
            return mre.column {
              padding = 10,
              mre.text { "Echo: " .. seen() },
              mre.textfield {
                key = "field",
                value = seen(),
                on_change = function(text) seen(text) end,
              },
            }
          end,
        }
        "#);

    let rect = app.ui().rect_of_key("field").expect("field");
    app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
        .expect("focus");

    // A real IME reports the complete text, selection and composition.
    assert!(app.set_text("héllo", 2, 4, "ll").expect("IME text"));
    assert_eq!(app.ui().input_text("field"), Some("héllo"));
    assert_eq!(app.focused_input_state(), Some(("héllo".to_string(), 2, 4)));
    assert!(
        app.runtime().tree().contains("Echo: héllo"),
        "on_change should publish the IME text"
    );

    // An unchanged report must not fire again or repaint.
    assert!(
        !app.set_text("héllo", 2, 4, "ll")
            .expect("unchanged IME text")
    );
}

#[test]
fn submitting_hands_control_back_to_the_app() {
    // A chat composer: while focused the editor owns the buffer, but the
    // moment a submit runs the app's cleared value has to win.
    let mut app = app(r#"
        local message = mre.signal("")
        local sent = mre.signal("")

        mre.app {
          view = function()
            return mre.column {
              padding = 10,
              mre.text { "Sent: " .. sent() },
              mre.textfield {
                key = "composer",
                value = message(),
                on_submit = function(text)
                  sent(text)
                  message("")
                end,
              },
            }
          end,
        }
        "#);

    let rect = app.ui().rect_of_key("composer").expect("composer");
    app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
        .expect("focus");
    app.insert_text("hello").expect("type");

    assert!(app.key("enter", false).expect("submit"));
    assert!(
        app.runtime().tree().contains("Sent: hello"),
        "tree was {}",
        app.runtime().tree()
    );
    assert_eq!(
        app.ui().input_text("composer"),
        Some(""),
        "the field should clear itself after a submit"
    );

    // And the composer is ready for the next message.
    app.insert_text("again").expect("type");
    assert_eq!(app.ui().input_text("composer"), Some("again"));
}

#[test]
fn a_card_border_follows_its_rounded_corners() {
    // A rounded card with a square outline shows the corners of the outline
    // past the corners of the card.
    let mut app = app(r##"
        mre.app {
          view = function()
            return mre.column {
              padding = 10,
              mre.card {
                radius = 16,
                width = 200, height = 100,
                background = "#121A2E",
                border = { color = "#506FC8", width = 2 },
                mre.text { "inside" },
              },
            }
          end,
        }
        "##);
    app.paint();

    let border = Rgba8::parse_hex("#506FC8").expect("colour");
    for (x, y) in [(10_u32, 10_u32), (209, 10), (10, 109), (209, 109)] {
        assert_ne!(
            app.canvas().pixel(x, y),
            Some(border),
            "the border reached the square corner at ({x}, {y})"
        );
    }
}

#[test]
fn a_changed_text_repaints_its_pixels() {
    // The damage diff compares the tree before and after a rebuild. If the
    // comparison happens after the tree was replaced, the new tree is
    // compared with itself, nothing is damaged, and the old pixels stay on
    // screen under the new text.
    let mut app = app(r#"
        local count = mre.signal(0)

        mre.app {
          view = function()
            return mre.column {
              padding = 10,
              mre.text { string.rep("W", 4) .. count(), size = 40 },
              mre.button {
                "Go",
                on_click = function() count(count() + 1) end,
              },
            }
          end,
        }
        "#);

    let rect = app.ui().layout_result().rect(NodeId::from_raw(2));
    assert!(rect.w > 10.0, "the text should have a width");

    let sample = |app: &App| -> Vec<u8> {
        let mut out = Vec::new();
        for step in 0..12 {
            let x = (rect.x + 4.0 + step as f32 * (rect.w - 8.0) / 12.0) as u32;
            let y = (rect.y + rect.h / 2.0) as u32;
            let pixel = app.canvas().pixel(x, y).expect("pixel");
            out.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
        }
        out
    };

    app.paint();
    let before = sample(&app);

    // Change the text: the pixels under it have to change with it.
    let button = app.ui().layout_result().rect(NodeId::from_raw(3));
    assert!(
        app.click(button.x + button.w / 2.0, button.y + button.h / 2.0)
            .expect("click"),
        "the button should run its handler"
    );
    app.paint();
    let after = sample(&app);

    assert_ne!(before, after, "the changed text should be repainted");
}

#[test]
fn a_selection_is_painted() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("hello").expect("type");
    app.paint();

    let rect = app.ui().rect_of_key("name").expect("field");
    let probe = ((rect.x + 14.0) as u32, (rect.y + rect.h / 2.0) as u32);
    let before = app.paint().pixel(probe.0, probe.1).expect("pixel");

    app.select_all();
    let after = app.paint().pixel(probe.0, probe.1).expect("pixel");
    assert_ne!(before, after, "selected text should be highlighted");
}

#[test]
fn undo_and_redo_restore_the_field() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("hello").expect("type");

    assert!(app.undo().expect("undo"));
    assert_eq!(app.ui().input_text("name"), Some(""));

    assert!(app.redo().expect("redo"));
    assert_eq!(app.ui().input_text("name"), Some("hello"));
}

#[test]
fn typing_after_an_undo_clears_the_redo_history() {
    let mut app = form();
    click_field(&mut app, "name");
    app.insert_text("hi").expect("type");
    assert!(app.undo().expect("undo"));
    app.insert_text("x").expect("type");
    assert!(!app.redo().expect("redo"));
    assert_eq!(app.ui().input_text("name"), Some("x"));
}

#[test]
fn a_composition_is_shown_but_not_committed() {
    let mut app = form();
    click_field(&mut app, "name");

    assert!(app.set_preedit("にほ").expect("preedit"));
    assert_eq!(
        app.ui().input_text("name"),
        Some(""),
        "a composition is not part of the value"
    );

    app.insert_text("日本").expect("commit");
    assert_eq!(app.ui().input_text("name"), Some("日本"));
}

#[test]
fn the_new_widgets_build_a_tree() {
    let source = r#"
mre.app {
  view = function()
    return mre.column {
      mre.tabs { items = { "One", "Two" }, active = 2 },
      mre.card { mre.badge { text = "3" } },
      mre.select { options = { "a", "b" }, value = "b", open = true },
      mre.dialog { open = true, mre.text { "Are you sure?" } },
    }
  end,
}
"#;
    let app = app(source);
    let tree = app.runtime().tree();
    assert!(tree.contains("One"), "tree was {tree}");
    assert!(tree.contains("Are you sure?"), "tree was {tree}");
    assert!(app.ui().node_count() > 10, "the widgets should build nodes");
}

#[test]
fn a_closed_dialog_builds_nothing_visible() {
    let source = r#"
mre.app {
  view = function()
    return mre.column {
      mre.text { "Behind" },
      mre.dialog { open = false, mre.text { "Hidden" } },
    }
  end,
}
"#;
    let app = app(source);
    let tree = app.runtime().tree();
    assert!(tree.contains("Behind"), "tree was {tree}");
    assert!(!tree.contains("Hidden"), "tree was {tree}");
}

fn table_app() -> App {
    let mut app = App::with_text_store(&table_source(), store()).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

#[test]
fn a_touch_drag_scrolls_the_container_under_the_finger() {
    let mut app = table_app();
    let before = app.ui().text_content();

    app.touch_start(200.0, 400.0);
    assert!(app.touch_move(200.0, 300.0).expect("touch move"));

    let after = app.ui().text_content();
    assert_ne!(before, after, "the rows should have moved");
    assert!(
        !after.contains("Instrument 00001"),
        "the window should have scrolled"
    );
}

#[test]
fn a_drag_is_not_a_click() {
    let mut app = app(COUNTER);
    let (x, y) = button_centre(&app);

    app.pointer_down(x, y);
    app.pointer_move(x, y + 30.0);
    assert!(!app.pointer_up(x, y + 30.0).expect("release"));

    assert!(
        app.runtime().tree().contains("Count: 0"),
        "a drag must not activate the button"
    );
}

#[test]
fn a_fast_flick_keeps_scrolling_and_then_stops() {
    let mut app = table_app();
    let before = app.ui().text_content();

    app.touch_start(200.0, 400.0);
    app.touch_move(200.0, 380.0).expect("touch move");
    // A fast flick upwards: the finger was moving up at 2000 px/s.
    app.touch_end(200.0, 360.0, 0.0, -2000.0)
        .expect("touch end");

    assert!(app.is_flinging(), "the flick should start a fling");

    let mut frames = 0;
    while app.is_flinging() && frames < 200 {
        app.tick(1.0 / 60.0).expect("tick");
        frames += 1;
    }

    assert!(frames > 1, "the fling should take more than one frame");
    assert!(!app.is_flinging(), "the fling should stop");
    assert_ne!(
        before,
        app.ui().text_content(),
        "the list should have moved"
    );
}

#[test]
fn a_slow_lift_does_not_fling() {
    let mut app = table_app();
    app.touch_start(200.0, 400.0);
    app.touch_move(200.0, 395.0).expect("touch move");
    app.touch_end(200.0, 390.0, 0.0, -10.0).expect("touch end");
    assert!(!app.is_flinging());
}

#[test]
fn hovering_a_node_updates_its_hover_signal() {
    let source = r#"
local over = mre.hover("target")
mre.app {
  view = function()
    return mre.column {
      mre.text { over() and "over" or "out" },
      mre.button { key = "target", "Target" },
    }
  end,
}
"#;
    let mut app = app(source);
    assert!(
        app.runtime().tree().contains("out"),
        "{}",
        app.runtime().tree()
    );

    let rect = app.ui().rect_of_key("target").expect("button");
    app.pointer_move(rect.x + 5.0, rect.y + rect.h / 2.0);
    app.frame().expect("frame");

    assert!(
        app.runtime().tree().contains("over"),
        "the hover signal should have changed: {}",
        app.runtime().tree()
    );

    // Leaving the node clears it again.
    app.pointer_move(1.0, 1.0);
    app.frame().expect("frame");
    assert!(
        app.runtime().tree().contains("out"),
        "the hover signal should have cleared: {}",
        app.runtime().tree()
    );
}

#[test]
fn a_node_that_appears_after_a_rebuild_is_sized_correctly() {
    // The tooltip only exists while the pointer is over the button, so its
    // node id used to belong to a different node. A measure cache that
    // survived the rebuild would size the tooltip with that node's old
    // measurement.
    let source = r#"
local over = mre.hover("target")
mre.app {
  view = function()
    return mre.column {
      mre.button { key = "target", "Target" },
      over() and mre.tooltip { key = "tip", text = "A tip" } or mre.spacer { size = 0 },
    }
  end,
}
"#;
    let mut app = app(source);
    let rect = app.ui().rect_of_key("target").expect("button");
    app.pointer_move(rect.x + 5.0, rect.y + rect.h / 2.0);
    app.frame().expect("frame");

    let tip = app.ui().rect_of_key("tip").expect("tooltip");
    assert!(tip.w > 10.0 && tip.h > 5.0, "tooltip was {tip:?}");
}

/// The bounding box of the painted regions, for tests that reason about "how
/// much of the screen a frame touched".
fn union(regions: &[Rect]) -> Rect {
    let first = regions.first().copied().expect("the frame painted nothing");
    regions.iter().copied().fold(first, |a, b| {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        let right = (a.x + a.w).max(b.x + b.w);
        let bottom = (a.y + a.h).max(b.y + b.h);
        Rect::new(x, y, right - x, bottom - y)
    })
}

#[test]
fn a_hover_repaints_only_the_hovered_region() {
    let mut app = app(COUNTER);
    let (x, y) = button_centre(&app);
    let surface = app.ui().layout_result().rect(NodeId::from_raw(1));
    let surface_area = surface.w * surface.h;

    assert!(app.pointer_move(x, y));
    let region = union(&app.paint_region());

    assert!(
        region.w * region.h < surface_area / 4.0,
        "a hover should cost its region, not the window: {region:?} of {surface:?}"
    );
    assert!(
        region.contains(mre_core::Point::new(x, y)),
        "the repainted region must cover the hovered node: {region:?}"
    );
}

#[test]
fn an_unchanged_frame_paints_nothing() {
    let mut app = app(COUNTER);
    app.frame().expect("frame");

    assert!(
        app.paint_region().is_empty(),
        "a frame with no change must not touch a single pixel"
    );
}

#[test]
fn a_keystroke_repaints_only_the_field() {
    let mut app = form();
    click_field(&mut app, "name");
    app.paint();
    let surface = app.ui().layout_result().rect(NodeId::from_raw(1));
    let surface_area = surface.w * surface.h;

    assert!(app.insert_text("A").expect("insert"));
    let region = union(&app.paint_region());

    assert!(
        region.w * region.h < surface_area / 4.0,
        "a keystroke should cost its field, not the window: {region:?} of {surface:?}"
    );
}

#[test]
fn a_resize_repaints_everything() {
    let mut app = app(COUNTER);
    app.paint();

    app.resize(Size::new(500.0, 400.0), 1.0, Insets::ZERO);
    let region = union(&app.paint_region());
    let surface = app.ui().layout_result().rect(NodeId::from_raw(1));

    assert!(
        region.w >= surface.w && region.h >= surface.h,
        "a resize invalidates the whole surface: {region:?} vs {surface:?}"
    );
}

#[test]
fn a_rebuild_repaints_only_what_changed() {
    let mut app = app(COUNTER);
    let (x, y) = button_centre(&app);
    app.paint();

    assert!(app.click(x, y).expect("click"));
    let region = union(&app.paint_region());
    let surface = app.ui().layout_result().rect(NodeId::from_raw(1));

    // The text "Count: 0" -> "Count: 1" changes one text run, but its old and
    // new widths differ, so the damage is at most the whole column — never the
    // empty space around it.
    assert!(
        region.w * region.h <= surface.w * surface.h,
        "damage must never exceed the surface: {region:?} of {surface:?}"
    );
    assert!(
        region.contains(mre_core::Point::new(x, y)),
        "the button text is inside the damage: {region:?}"
    );
}

const SCROLL_TABLE: &str = r##"
local offset = mre.signal(0)
local ROW_HEIGHT = 20

local function row(index)
  return mre.row {
    background = index % 2 == 0 and "#1c1c1c" or "#202020",
    height = ROW_HEIGHT, width = "100%",
    absolute = true,
    top = index * ROW_HEIGHT - offset(),
    mre.text { "row " .. index, size = 12 },
  }
end

mre.app {
  view = function()
    local rows = {}
    for index = 0, 11 do
      rows[#rows + 1] = row(index)
    end
    return mre.column {
      background = "#141414",
      mre.row { background = "#2a2a2a", height = 24,
        mre.text { "header", size = 12 } },
      mre.scroll {
        height = 200, width = "100%",
        on_scroll = function(_, dy)
          offset(math.max(0, math.min(40, offset() + dy)))
        end,
        rows,
      },
    }
  end,
}
"##;

fn scroll_app() -> App {
    let mut app = App::with_text_store(SCROLL_TABLE, store()).expect("app");
    app.resize(Size::new(400.0, 300.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

#[test]
fn a_blitted_scroll_matches_a_full_repaint_pixel_for_pixel() {
    let mut scrolled = scroll_app();
    let mut reference = scroll_app();

    for _ in 0..10 {
        assert!(scrolled.scroll(200.0, 200.0, 0.0, 4.0).expect("scroll"));
        scrolled.frame().expect("frame");
        scrolled.paint();
    }
    assert!(reference.scroll(200.0, 200.0, 0.0, 40.0).expect("scroll"));
    reference.frame().expect("frame");
    reference.paint();

    let scrolled_pixels = scrolled.canvas().pixels();
    let reference_pixels = reference.canvas().pixels();
    assert_eq!(scrolled_pixels.len(), reference_pixels.len());
    let mismatches = scrolled_pixels
        .iter()
        .zip(reference_pixels.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        mismatches, 0,
        "a blitted scroll must produce the same pixels as repainting"
    );
}

#[test]
fn a_blitted_scroll_repaints_only_the_bands() {
    let mut app = scroll_app();
    let surface = app.ui().layout_result().rect(NodeId::from_raw(1));
    let surface_area = surface.w * surface.h;

    assert!(app.scroll(200.0, 200.0, 0.0, 4.0).expect("scroll"));
    app.frame().expect("frame");
    let bands = app.paint_region();

    let painted_area: f32 = bands.iter().map(|band| band.w * band.h).sum();
    assert!(
        painted_area < surface_area / 3.0,
        "a 4px scroll should repaint two bands, not the window: \
         {painted_area:.0} of {:.0}",
        surface_area
    );
}

#[test]
fn timer_writes_reach_the_rust_tree() {
    let mut app = app(r#"
        local label = mre.signal("waiting")
        mre.after(10, function() label("done") end)
        mre.app {
          view = function()
            return mre.column { mre.text { label() } }
          end,
        }
        "#);
    assert!(app.runtime().tree().contains("waiting"));
    app.tick(0.02).expect("tick fires the timer");
    assert!(
        app.runtime().tree().contains("done"),
        "a timer write must rebuild the Rust view, not only Lua: {}",
        app.runtime().tree()
    );
}

#[test]
fn overlays_anchor_to_the_full_window_under_insets() {
    let mut app = app(r#"
        mre.app {
          view = function()
            return mre.column {
              mre.dialog { key = "scrim", open = true, mre.text { "modal" } },
            }
          end,
        }
        "#);
    app.resize(
        Size::new(400.0, 640.0),
        1.0,
        Insets::new(40.0, 10.0, 20.0, 5.0),
    );
    app.frame().expect("frame");
    let rect = app.ui().rect_of_key("scrim").expect("scrim");
    assert!(
        rect.x <= 0.0,
        "scrim must bleed past the left inset: {rect:?}"
    );
    assert!(
        rect.y <= 0.0,
        "scrim must bleed past the top inset: {rect:?}"
    );
    assert!(rect.w >= 400.0, "scrim must cover the full width: {rect:?}");
    assert!(
        rect.h >= 640.0,
        "scrim must cover the full height: {rect:?}"
    );
}

#[test]
fn unknown_properties_are_reported_to_lua() {
    let app = app(r#"
        mre.app {
          view = function()
            return mre.column { mre.text { "hello", wigth = 12 } }
          end,
        }
        "#);
    let diagnostics = app
        .runtime()
        .eval::<String>("table.concat(mre.diagnostics(), '|')")
        .expect("diagnostics");
    assert!(
        diagnostics.contains("wigth"),
        "unknown props must be logged, got {diagnostics}"
    );
}

#[test]
fn lua_can_switch_the_palette_at_runtime() {
    let mut app = app(r#"
        mre.set_theme("dark")
        mre.app { view = function() return mre.column { mre.text { "themed" } } end }
        "#);
    app.frame().expect("frame applies the theme");
    assert_eq!(app.runtime().eval::<String>("mre.theme()").unwrap(), "dark");
    assert!(app.runtime().theme_request().borrow().is_none());
}
