//! Draws the baseline of every text run over the app, so any text that does
//! not sit on a common line is visible.
use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};

fn main() {
    let mut engine = TextEngine::new();
    engine.load_system();
    let _ = FontDesc::new("Roboto");
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/todo.lua".into());
    let source = std::fs::read_to_string(&path).expect("app");
    let mut app = App::with_text_store(&source, TextStore::new(engine)).expect("app");
    app.resize(Size::new(900.0, 760.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();

    for (content, rect, baseline) in app.ui().text_baselines() {
        println!(
            "{content:?} rect=({:.1},{:.1},{:.1},{:.1}) baseline_y={:.2}",
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            rect.y + baseline
        );
    }
}
