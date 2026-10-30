//! Renders a Lua app to a PNG, headlessly.
//!
//! ```text
//! cargo run -p mre-runtime --example render -- examples/dashboard.lua /tmp/app.png 900 760
//! ```
//!
//! This is how the engine is tested without a window, and how CI produces visual
//! diffs on machines that have no display.

use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};

fn main() {
    let mut args = std::env::args().skip(1);
    let lua_path = args
        .next()
        .unwrap_or_else(|| "examples/dashboard.lua".to_string());
    let out = args
        .next()
        .unwrap_or_else(|| "/tmp/opencode/app.png".to_string());
    let width: f32 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(900.0);
    let height: f32 = args
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(760.0);

    let source = std::fs::read_to_string(&lua_path).expect("read lua source");

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

    let mut app = App::with_text_store(&source, TextStore::new(engine)).expect("app");
    app.resize(Size::new(width, height), 1.0, Insets::ZERO);
    app.frame().expect("frame");

    // `MRE_SCROLL=<pixels>` renders a scrolled frame, which is how scroll
    // clipping and virtualisation are checked visually in CI artifacts.
    // `MRE_TICKS=<n>` advances n frames at 60 Hz before painting, which is how
    // mid-animation states are captured for visual checks.
    if let Ok(ticks) = std::env::var("MRE_TICKS") {
        if let Ok(ticks) = ticks.parse::<u32>() {
            for _ in 0..ticks {
                app.tick(1.0 / 60.0).expect("tick");
            }
        }
    }

    if let Ok(scroll) = std::env::var("MRE_SCROLL") {
        if let Ok(dy) = scroll.parse::<f32>() {
            app.scroll(width / 2.0, height / 2.0, 0.0, dy)
                .expect("scroll");
        }
    }

    let canvas = app.paint();
    canvas.save_png(&out).expect("write png");
    println!(
        "wrote {out} ({}x{}) nodes={} rebuilds={}",
        canvas.width(),
        canvas.height(),
        app.stats().nodes,
        app.stats().rebuilds
    );
}
