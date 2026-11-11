//! Text editing: the keys, the mouse and the caret.
//!
//! These are the behaviours a user feels directly, so they are tested from
//! the outside: a key goes in, the field's text and selection come out.

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

/// A field with a handler, so a change can be observed.
const FIELD: &str = r#"
    local value = mre.signal("")

    mre.app {
      view = function()
        return mre.column {
          padding = 12,
          mre.textfield {
            key = "field",
            value = value(),
            on_change = function(text) value(text) end,
          },
        }
      end,
    }
"#;

fn app() -> App {
    let mut app = App::with_text_store(FIELD, store()).expect("app");
    app.resize(Size::new(400.0, 200.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

fn focus(app: &mut App) {
    let rect = app.ui().rect_of_key("field").expect("field");
    app.click(rect.x + 20.0, rect.y + rect.h / 2.0)
        .expect("click");
}

fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        ..Modifiers::default()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::default()
    }
}

fn ctrl_shift() -> Modifiers {
    Modifiers {
        ctrl: true,
        shift: true,
        ..Modifiers::default()
    }
}

fn text(app: &App) -> String {
    app.ui().input_text("field").unwrap_or_default().to_string()
}

fn selected(app: &App) -> String {
    app.ui().selected_text().unwrap_or_default()
}

#[test]
fn control_and_backspace_delete_a_word() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    app.key_with("backspace", ctrl()).expect("delete word");
    assert_eq!(text(&app), "ship the ");

    app.key_with("backspace", ctrl()).expect("delete word");
    assert_eq!(text(&app), "ship ");
}

#[test]
fn control_and_delete_remove_the_next_word() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    // Back to the start of the text.
    app.key_with("home", Modifiers::default()).expect("home");
    app.key_with("delete", ctrl()).expect("delete word");
    assert_eq!(text(&app), "the demo");
}

#[test]
fn control_and_the_arrows_move_by_word() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    app.key_with("left", ctrl()).expect("word left");
    app.key_with("left", ctrl()).expect("word left");
    app.key_with("left", ctrl()).expect("word left");
    // The caret is now at the start of "ship".
    app.insert_text(">").expect("type");
    assert_eq!(text(&app), ">ship the demo");

    app.key_with("right", ctrl()).expect("word right");
    app.key_with("right", ctrl()).expect("word right");
    app.insert_text("<").expect("type");
    assert_eq!(text(&app), ">ship the <demo");
}

#[test]
fn shift_and_the_arrows_extend_the_selection() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    // Two characters back from the end of "demo" is "mo".
    app.key_with("left", shift()).expect("extend");
    app.key_with("left", shift()).expect("extend");
    assert_eq!(selected(&app), "mo");

    // One forward shrinks the selection again.
    app.key_with("right", shift()).expect("extend");
    assert_eq!(selected(&app), "o");
}

#[test]
fn control_and_shift_select_words() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    // One word back from the end is the last word.
    app.key_with("left", ctrl_shift()).expect("select word");
    assert_eq!(selected(&app), "demo");

    // Another one takes the word before it, and the space between.
    app.key_with("left", ctrl_shift()).expect("select word");
    assert_eq!(selected(&app), "the demo");

    // And forward again shrinks the selection to a single word.
    app.key_with("right", ctrl_shift()).expect("shrink");
    assert_eq!(selected(&app), "demo");
}

#[test]
fn control_and_a_selects_everything() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    assert!(app.key_with("a", ctrl()).expect("select all"));
    assert_eq!(selected(&app), "ship the demo");
}

#[test]
fn an_arrow_keeps_a_selection_and_collapses_it() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");
    app.key_with("a", ctrl()).expect("select all");

    // Left collapses the selection to its start.
    app.key_with("left", Modifiers::default()).expect("left");
    assert_eq!(selected(&app), "");

    app.key_with("a", ctrl()).expect("select all");
    // Right collapses it to its end.
    app.key_with("right", Modifiers::default()).expect("right");
    assert_eq!(selected(&app), "");
}

#[test]
fn a_double_click_selects_the_word_under_the_pointer() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    let rect = app.ui().rect_of_key("field").expect("field");
    // The middle of the field, which is inside the word "the".
    let x = rect.x + 45.0;
    let y = rect.y + rect.h / 2.0;

    app.pointer_down_with(x, y, Modifiers::default());
    app.pointer_up(x, y).expect("release");
    // The second press, close in time and space, is a double click.
    app.pointer_down_with(x, y, Modifiers::default());

    let selection = selected(&app);
    assert!(
        ["ship", "the", "demo"].contains(&selection.as_str()),
        "a double click should select a whole word, got {selection:?}"
    );
}

#[test]
fn a_triple_click_selects_everything() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    let rect = app.ui().rect_of_key("field").expect("field");
    let x = rect.x + 45.0;
    let y = rect.y + rect.h / 2.0;

    for _ in 0..3 {
        app.pointer_down_with(x, y, Modifiers::default());
        app.pointer_up(x, y).expect("release");
    }

    assert_eq!(selected(&app), "ship the demo");
}

#[test]
fn a_drag_selects_the_text_it_covers() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    let rect = app.ui().rect_of_key("field").expect("field");
    let y = rect.y + rect.h / 2.0;
    app.pointer_down_with(rect.x + 8.0, y, Modifiers::default());
    app.pointer_drag(rect.x + 30.0, y).expect("drag");
    let short = selected(&app);

    app.pointer_drag(rect.x + 90.0, y).expect("drag");
    let long = selected(&app);

    assert!(
        text(&app).starts_with(&long),
        "the selection should be a prefix of the text, got {long:?}"
    );
    assert!(
        long.len() > short.len(),
        "dragging further should select more, got {short:?} then {long:?}"
    );
    assert!(
        short.len() > 1,
        "the drag should select more than a character"
    );
}

#[test]
fn shift_and_a_click_extends_the_selection() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ship the demo").expect("type");

    let rect = app.ui().rect_of_key("field").expect("field");
    let y = rect.y + rect.h / 2.0;
    app.pointer_down_with(rect.x + 8.0, y, Modifiers::default());
    app.pointer_up(rect.x + 8.0, y).expect("release");
    // Shift and a click extends the selection from the caret.
    app.pointer_down_with(rect.x + 62.0, y, shift());

    assert_eq!(selected(&app), "ship the ");
}

#[test]
fn the_caret_blinks_while_a_field_is_focused() {
    let mut app = app();
    focus(&mut app);
    assert!(app.has_focused_input(), "the field should be focused");

    // The caret starts visible.
    let mut visible = Vec::new();
    for _ in 0..12 {
        app.tick(0.1).expect("tick");
        visible.push(app.ui().caret_visible("field"));
    }
    assert!(
        visible.contains(&true) && visible.contains(&false),
        "the caret should blink, saw {visible:?}"
    );
}

#[test]
fn typing_makes_the_caret_visible_again() {
    let mut app = app();
    focus(&mut app);
    app.insert_text("ab").expect("type");

    // Half a second later the caret is in its hidden phase.
    for _ in 0..6 {
        app.tick(0.1).expect("tick");
    }
    assert!(
        !app.ui().caret_visible("field"),
        "the caret should be hidden"
    );

    app.insert_text("c").expect("type");
    assert!(
        app.ui().caret_visible("field"),
        "typing should bring the caret back"
    );
}
