//! The Android example is a clickable app, so test it by clicking.
//!
//! These are the exact interactions a finger performs on a phone: hit-test a
//! button, run its Lua handler, rebuild, and check what the user now sees. No
//! emulator is needed to prove the example is interactive.

use mre_core::{Insets, Size};
use mre_runtime::App;
use mre_text::{FontDesc, TextEngine, TextStore};
use mre_widgets::Role;

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

fn app() -> App {
    let source = include_str!("../../examples/android_gallery.lua");
    let mut app = App::with_text_store(source, store()).expect("gallery");
    // A small phone-sized logical viewport, like a 360dp-wide device.
    app.resize(Size::new(400.0, 640.0), 1.0, Insets::ZERO);
    app.frame().expect("frame");
    app.paint();
    app
}

/// Clicks a button by its stable Lua key, through the same hit test a touch
/// event uses. The bounds come from the accessibility tree, not from a hard
/// coded pixel, so a layout change cannot silently break the test.
fn click_key(app: &mut App, key: &str) -> bool {
    let node = app
        .semantics()
        .into_iter()
        .find(|node| node.key.as_deref() == Some(key) && node.role == Role::Button)
        .unwrap_or_else(|| panic!("no button with key {key:?} in the semantic tree"));
    let x = node.rect.x + node.rect.w / 2.0;
    let y = node.rect.y + node.rect.h / 2.0;
    let hit = app.click(x, y).expect("click handler");
    app.frame().expect("frame after click");
    app.paint();
    hit
}

fn shows(app: &App, text: &str) {
    let tree = app.runtime().tree();
    assert!(
        tree.contains(text),
        "expected {text:?} in view tree:\n{tree}"
    );
}

#[test]
fn gallery_pages_are_reachable_by_click() {
    let mut app = app();
    shows(&app, "MRE for Android");
    shows(&app, "Tap me");

    assert!(click_key(&mut app, "page-checks"));
    shows(&app, "Click-to-run checks");
    shows(&app, "not run yet");

    assert!(click_key(&mut app, "page-about"));
    shows(&app, "MRE on Android");

    assert!(click_key(&mut app, "page-controls"));
    shows(&app, "Tap me");
}

#[test]
fn the_clickable_checks_report_all_passed() {
    let mut app = app();
    assert!(click_key(&mut app, "page-checks"));
    assert!(click_key(&mut app, "run-checks"));
    shows(&app, "5/5 passed");
}

#[test]
fn counter_button_and_reset_work() {
    let mut app = app();
    assert!(click_key(&mut app, "tap"));
    shows(&app, "Tapped 1 times");
    assert!(click_key(&mut app, "tap"));
    shows(&app, "Tapped 2 times");
    assert!(click_key(&mut app, "reset"));
    shows(&app, "Tapped 0 times");
}
