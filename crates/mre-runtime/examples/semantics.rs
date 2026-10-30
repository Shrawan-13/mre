//! Prints the accessibility tree for an app.
//!
//! ```text
//! cargo run -p mre-runtime --example semantics -- examples/form.lua
//! ```
//!
//! This is the surface a platform accessibility bridge consumes: roles, names,
//! values and bounds, all derived from the same view tree that paints.

use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/form.lua".to_string());
    let source = std::fs::read_to_string(&path)?;

    let mut engine = TextEngine::new();
    engine.add_font_file(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts/Roboto-Regular.ttf"
        ),
        FontDesc::new("Roboto"),
    )?;

    let mut app = App::with_text_store(&source, TextStore::new(engine))?;
    app.resize(Size::new(420.0, 400.0), 1.0, Insets::ZERO);
    app.frame()?;

    for node in app.semantics() {
        println!(
            "{:>11}  {:<6} {:>4.0},{:<4.0} {:>4.0}x{:<4.0}  {}{}{}",
            format!("{:?}", node.role),
            node.key.as_deref().unwrap_or("-"),
            node.rect.x,
            node.rect.y,
            node.rect.w,
            node.rect.h,
            node.label,
            node.value
                .as_deref()
                .filter(|_| !node.label.is_empty())
                .map(|value| format!(" = {value:?}"))
                .unwrap_or_default(),
            if node.focused {
                "  [focused]"
            } else if node.focusable {
                "  [focusable]"
            } else {
                ""
            }
        );
    }

    Ok(())
}
