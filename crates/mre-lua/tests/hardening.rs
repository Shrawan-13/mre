//! Determinism and diagnostics guarantees of the Lua runtime.

use mre_lua::Runtime;

#[test]
fn safe_area_is_a_reactive_signal() {
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec(
            "changes = 0\n\
             mre.effect(function() mre.safe_area(); changes = changes + 1 end)",
        )
        .expect("subscribe to safe area");
    assert_eq!(runtime.eval::<i64>("changes").unwrap(), 1);

    runtime.set_safe_area(24.0, 8.0, 16.0, 0.0);
    runtime.flush().expect("flush");
    assert_eq!(runtime.eval::<f64>("mre.safe_area().top").unwrap(), 24.0);
    assert!(runtime.eval::<i64>("changes").unwrap() >= 2);

    // An unchanged inset must not wake anything.
    let before = runtime.eval::<i64>("changes").unwrap();
    runtime.set_safe_area(24.0, 8.0, 16.0, 0.0);
    runtime.flush().expect("flush");
    assert_eq!(runtime.eval::<i64>("changes").unwrap(), before);
}

#[test]
fn the_clock_only_advances_on_tick() {
    let runtime = Runtime::new().expect("runtime");
    assert_eq!(runtime.now(), 0.0);
    assert_eq!(runtime.eval::<f64>("mre.now()").unwrap(), 0.0);
    runtime.tick(0.25).expect("tick");
    assert_eq!(runtime.now(), 0.25);
    assert_eq!(runtime.eval::<f64>("mre.now()").unwrap(), 0.25);
    runtime.tick(f32::NAN).expect("ignore invalid dt");
    assert_eq!(runtime.now(), 0.25);
}

#[test]
fn theme_requests_are_shared_with_the_host() {
    let runtime = Runtime::new().expect("runtime");
    assert_eq!(runtime.eval::<String>("mre.theme()").unwrap(), "light");
    runtime.exec("mre.set_theme('dark')").expect("set theme");
    assert_eq!(runtime.eval::<String>("mre.theme()").unwrap(), "dark");
    assert_eq!(
        runtime.theme_request().borrow_mut().take().as_deref(),
        Some("dark")
    );
}

#[test]
fn diagnostics_are_readable_from_lua() {
    let runtime = Runtime::new().expect("runtime");
    runtime.set_diagnostics(vec!["unknown property 'wigth' on node".to_string()]);
    assert_eq!(runtime.eval::<i64>("#mre.diagnostics()").unwrap(), 1);
    assert!(
        runtime
            .eval::<String>("mre.diagnostics()[1]")
            .unwrap()
            .contains("wigth")
    );
}

#[test]
fn an_error_boundary_shows_a_fallback() {
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec(
            "tree = mre.boundary {\n\
               view = function() error('broken subtree') end,\n\
               fallback = function() return mre.text('Recovered.') end,\n\
             }",
        )
        .expect("boundary");
    assert_eq!(runtime.eval::<String>("tree.text").unwrap(), "Recovered.");
}
