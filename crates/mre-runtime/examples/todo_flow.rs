//! Drives the todo example through its whole interaction flow, headlessly.
//!
//! ```text
//! cargo run -p mre-runtime --example todo_flow
//! ```
//!
//! Types todos, submits them with Enter, completes one, deletes another, and
//! writes a PNG of every stage to /tmp. This is the "does a real app work"
//! walk: keyboard input, Enter-to-submit, a signal-driven rebuild per action,
//! clicks on composed widgets.

use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};

fn main() {
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

    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/todo.lua"
    ))
    .expect("todo example");

    let mut app = App::with_text_store(&source, TextStore::new(engine)).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint()
        .save_png("/tmp/todo-1-empty.png")
        .expect("write png");

    let stage = |app: &mut App, name: &str| {
        app.frame().expect("frame");
        let regions = app.paint_region();
        println!("[stage {name}] painted {} regions", regions.len());
        for region in &regions {
            println!(
                "   band x={:.1} y={:.1} w={:.1} h={:.1}",
                region.x, region.y, region.w, region.h
            );
        }
        app.paint().save_png(name).expect("write png");
    };

    // Focus the field, then type two todos and submit each with Enter.
    let field = app.ui().rect_of_key("draft").expect("draft field");
    app.click(field.x + field.w / 2.0, field.y + field.h / 2.0)
        .expect("click");
    app.insert_text("Ship the demo").expect("type");
    println!(
        "field buffer: {:?} | stats: {:?}",
        app.ui().input_text("draft"),
        app.stats()
    );
    app.key("enter", false).expect("enter");
    println!(
        "after enter: stats {:?} | text: {}",
        app.stats(),
        app.ui().text_content()
    );
    stage(&mut app, "/tmp/todo-2-added.png");

    app.insert_text("Push to GitHub").expect("type");
    app.key("enter", false).expect("enter");
    stage(&mut app, "/tmp/todo-3-added.png");

    println!(
        "after adding, text: {} | keys todo-2: {:?}",
        app.ui().text_content(),
        app.ui().rect_of_key("todo-2")
    );

    // Complete the second todo.
    let rect = app.ui().rect_of_key("todo-2").expect("toggle target");
    assert!(
        app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            .expect("click"),
        "the toggle button should run its handler"
    );
    stage(&mut app, "/tmp/todo-4-done.png");

    // Open the confirmation dialog.
    let rect = app.ui().rect_of_key("clear").expect("clear button");
    assert!(
        app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            .expect("click"),
        "the clear button should open the dialog"
    );
    stage(&mut app, "/tmp/todo-6-dialog.png");

    // A full repaint of the same frame, for comparison.
    app.repaint_all();
    app.frame().expect("frame");
    app.paint()
        .save_png("/tmp/todo-7-dialog-full.png")
        .expect("write png");

    // Delete the first one.
    let rect = app.ui().rect_of_key("remove-1").expect("remove target");
    assert!(
        app.click(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0)
            .expect("click"),
        "the remove button should run its handler"
    );
    stage(&mut app, "/tmp/todo-5-removed.png");

    let text = app.ui().text_content();
    println!("final state: {text}");
    println!(
        "nodes={} rebuilds={}",
        app.ui().node_count(),
        app.stats().rebuilds
    );
}
