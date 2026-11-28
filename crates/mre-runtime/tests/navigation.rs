//! The engine only owns one thing in navigation: translating a platform back
//! event into the portable Lua chain. These tests cover that boundary and the
//! keyed-screen behaviour it relies on.

use mre_runtime::{App, Modifiers};

const NAV: &str = include_str!("../../../framework/nav.lua");

const SOURCE: &str = r#"
    mre.load('framework/nav')

    local function Home(ctx)
        return mre.column {
            mre.text { 'home' },
            mre.textfield { key = 'field', value = '' },
        }
    end

    local function Detail(ctx)
        return mre.text { 'detail ' .. tostring(ctx.params.id) }
    end

    nav = mre.nav {
        routes = { home = Home, detail = Detail },
        initial = 'home',
        transition_ms = 0,
        persist = 'runtime.nav',
    }

    mre.app { view = function() return nav:view() end }
"#;

fn app() -> App {
    let app = App::with_lua_loader(SOURCE, |name| {
        (name == "framework/nav").then(|| NAV.to_string())
    })
    .expect("app with navigation");
    let mut app = app;
    app.resize(
        mre_core::Size::new(400.0, 800.0),
        1.0,
        mre_core::Insets::ZERO,
    );
    app.frame().expect("first frame");
    app
}

#[test]
fn back_pops_one_screen_and_restores_the_previous_one() {
    let mut app = app();
    assert!(app.runtime().tree().contains("home"));

    app.runtime()
        .exec("nav:push('detail', { id = 4 })")
        .unwrap();
    app.frame().unwrap();
    assert!(app.runtime().tree().contains("detail 4"));

    assert!(app.back().unwrap());
    assert!(app.runtime().tree().contains("home"));
    assert!(!app.runtime().tree().contains("detail 4"));
}

#[test]
fn app_handler_is_the_last_chance_before_platform_exit() {
    let source =
        format!("{SOURCE}\nmre.on_back(function() mre.__hooked = true; return true end, -5)");
    let mut app = App::with_lua_loader(&source, |name| {
        (name == "framework/nav").then(|| NAV.to_string())
    })
    .unwrap();
    app.resize(
        mre_core::Size::new(400.0, 800.0),
        1.0,
        mre_core::Insets::ZERO,
    );
    app.frame().unwrap();

    assert!(app.back().unwrap());
    assert!(app.runtime().eval::<bool>("mre.__hooked").unwrap());
}

#[test]
fn a_focused_editor_consumes_back_before_navigation() {
    let mut app = app();
    app.runtime()
        .exec("nav:push('detail', { id = 5 })")
        .unwrap();
    app.frame().unwrap();

    // Tab focuses the first text field in Home; the detail route has none, so
    // pop back first and let the shell field take focus.
    app.back().unwrap();
    app.key_with("tab", Modifiers::default()).unwrap();
    assert!(app.focused().is_some(), "tab must focus the text field");

    assert!(app.back().unwrap());
    assert!(
        app.focused().is_none(),
        "back must dismiss the editor first"
    );
    assert!(
        app.runtime().tree().contains("home"),
        "the route must not change while an editor was focused"
    );
}
