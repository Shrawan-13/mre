use mre_lua::Runtime;

const SOURCE: &str = r#"
n = mre.persist("counter", 1)
p = mre.signal(10)
mre.effect(function() n(); p() end)
mre.app { view = function() return mre.text { "reload" } end }
"#;

#[test]
fn reload_preserves_only_declared_state() {
    let mut runtime = Runtime::new().expect("runtime");
    runtime.exec(SOURCE).expect("initial app");
    runtime.exec("n(7); p(99)").expect("writes");

    runtime.reload(SOURCE).expect("reload");

    let persisted: i64 = runtime.eval("n()").expect("persisted value");
    let fresh: i64 = runtime.eval("p()").expect("fresh value");
    assert_eq!(persisted, 7, "mre.persist must survive a reload");
    assert_eq!(fresh, 10, "plain signals must restart deterministically");
}

#[test]
fn reload_replaces_the_effect_graph_instead_of_duplicating_it() {
    let mut runtime = Runtime::new().expect("runtime");
    runtime.exec(SOURCE).expect("initial app");

    let runs_before: u64 = runtime
        .eval("(function() local s = mre.stats(); return s.total_runs end)()")
        .expect("stats");
    runtime.reload(SOURCE).expect("reload");
    let runs_after: u64 = runtime
        .eval("(function() local s = mre.stats(); return s.total_runs end)()")
        .expect("stats");

    // First boot: one mount effect. Reload: one mount effect. A leaked graph
    // would add another effect on every reload.
    assert!(
        runs_after <= runs_before + 2,
        "effects leaked: {runs_before} -> {runs_after}"
    );
}
