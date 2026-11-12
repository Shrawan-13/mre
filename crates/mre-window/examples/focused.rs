//! Runs an app in a window with a field focused, so the caret can be watched.
//!
//! ```text
//! cargo run -p mre-window --example focused -- examples/todo.lua
//! ```
//!
//! The pointer cannot be moved from here, so the field is focused by the
//! same path a click takes: press, release.

use mre_core::{Insets, Size};
use mre_runtime::App;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/todo.lua".to_string());
    let source = std::fs::read_to_string(&path).expect("read the app");
    let mut app = App::new(&source).expect("app");

    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();

    if let Some(rect) = app.ui().rect_of_key("draft") {
        let x = rect.x + 40.0;
        let y = rect.y + rect.h / 2.0;
        app.pointer_down_with(x, y, mre_runtime::Modifiers::default());
        app.pointer_up(x, y).expect("release");
        app.insert_text("Ship the demo").expect("type");
    }

    if let Err(error) = mre_window::run(app, "MRE") {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
