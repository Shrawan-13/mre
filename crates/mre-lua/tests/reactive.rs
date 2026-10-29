//! Behavioural tests for the reactive graph.
//!
//! These are the tests that make the central claim of the architecture checkable:
//! *a frame in which nothing changed runs no Lua code*, and *a change re-runs only
//! the computations that depend on it*.

use mre_lua::Runtime;

#[test]
fn signal_write_rerenders_the_mounted_tree() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        local count = mre.signal(0)
        mre.__count = count

        mre.mount(function()
            return mre.column {
                gap = 16,
                mre.text { "Clicked " .. count() .. " times" },
                mre.button { label = "Increment" },
            }
        end)
        "#,
    )
    .unwrap();

    assert_eq!(rt.tree(), r#"(column (text "Clicked 0 times") (button))"#);

    rt.exec("mre.__count(3)").unwrap();
    rt.flush().unwrap();

    assert_eq!(rt.tree(), r#"(column (text "Clicked 3 times") (button))"#);
}

#[test]
fn writing_the_same_value_does_not_invalidate() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        local s = mre.signal(7)
        mre.__s = s
        mre.__runs = 0
        mre.effect(function() s(); mre.__runs = mre.__runs + 1 end)
        "#,
    )
    .unwrap();

    assert_eq!(rt.eval::<i64>("mre.__runs").unwrap(), 1);

    rt.exec("mre.__s(7)").unwrap();
    rt.flush().unwrap();

    assert_eq!(rt.eval::<i64>("mre.__runs").unwrap(), 1);
    assert_eq!(rt.stats().last_flush_runs, 0);
}

#[test]
fn only_dependent_computations_rerun() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        local a = mre.signal(0)
        local b = mre.signal(0)
        mre.__a = a
        mre.__b = b
        mre.__runs = { a = 0, b = 0 }

        mre.effect(function() a(); mre.__runs.a = mre.__runs.a + 1 end)
        mre.effect(function() b(); mre.__runs.b = mre.__runs.b + 1 end)
        "#,
    )
    .unwrap();

    assert_eq!(rt.eval::<i64>("mre.__runs.a").unwrap(), 1);
    assert_eq!(rt.eval::<i64>("mre.__runs.b").unwrap(), 1);

    rt.exec("mre.__a(1)").unwrap();
    rt.flush().unwrap();

    assert_eq!(
        rt.eval::<i64>("mre.__runs.a").unwrap(),
        2,
        "the effect reading `a` must re-run"
    );
    assert_eq!(
        rt.eval::<i64>("mre.__runs.b").unwrap(),
        1,
        "the effect reading only `b` must not run"
    );
    assert_eq!(rt.stats().last_flush_runs, 1);
}

#[test]
fn writes_are_batched_into_a_single_rerun() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        local width = mre.signal(0)
        local height = mre.signal(0)
        mre.__resize = function(w, h) width(w); height(h) end
        mre.__runs = 0

        mre.effect(function()
            width(); height()
            mre.__runs = mre.__runs + 1
        end)
        "#,
    )
    .unwrap();

    assert_eq!(rt.eval::<i64>("mre.__runs").unwrap(), 1);

    rt.exec("mre.__resize(100, 50)").unwrap();
    rt.flush().unwrap();

    assert_eq!(
        rt.eval::<i64>("mre.__runs").unwrap(),
        2,
        "two writes before one flush must produce exactly one rerun"
    );
}

#[test]
fn idle_flush_runs_no_lua() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        local n = mre.signal(1)
        mre.effect(function() n() end)
        "#,
    )
    .unwrap();

    let before = rt.stats().total_runs;
    for _ in 0..10 {
        rt.flush().unwrap();
    }
    assert_eq!(
        rt.stats().total_runs,
        before,
        "flushing an idle graph must not execute any computation"
    );
    assert_eq!(rt.stats().last_flush_runs, 0);
}

#[test]
fn peek_reads_without_subscribing() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        local s = mre.signal(10)
        mre.__s = s
        mre.__seen = nil
        mre.__runs = 0

        mre.effect(function()
            mre.__seen = mre.peek(s)
            mre.__runs = mre.__runs + 1
        end)
        "#,
    )
    .unwrap();

    assert_eq!(rt.eval::<i64>("mre.__seen").unwrap(), 10);
    assert_eq!(rt.eval::<i64>("mre.__runs").unwrap(), 1);

    rt.exec("mre.__s(11)").unwrap();
    rt.flush().unwrap();

    assert_eq!(
        rt.eval::<i64>("mre.__runs").unwrap(),
        1,
        "peek must not create a subscription"
    );
    assert_eq!(rt.eval::<i64>("mre.__seen").unwrap(), 10);
}

#[test]
fn dependency_cycle_is_reported_not_hung() {
    let rt = Runtime::new().unwrap();
    let err = rt
        .exec(
            r#"
            local s = mre.signal(0)
            mre.effect(function()
                s(s() + 1)
            end)
            "#,
        )
        .expect_err("a self-incrementing effect must be detected as a cycle");

    let message = err.to_string();
    assert!(
        message.contains("cycle"),
        "error should mention the cycle, got: {message}"
    );
}

#[test]
fn errors_in_a_computation_propagate_and_leave_the_graph_usable() {
    let rt = Runtime::new().unwrap();

    rt.exec(
        r#"
        local s = mre.signal(0)
        mre.__s = s
        mre.effect(function()
            if s() > 0 then error("boom") end
        end)
        "#,
    )
    .unwrap();

    rt.exec("mre.__s(1)").unwrap();
    let err = rt.flush().unwrap_err();
    assert!(err.to_string().contains("boom"));

    // The graph must be left in a usable state: clearing the fault re-runs the
    // same computation and succeeds.
    rt.exec("mre.__s(0)").unwrap();
    rt.flush().unwrap();
}

#[test]
fn tree_dump_reports_text_and_nesting() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        mre.mount(function()
            return mre.row {
                mre.text "left",
                mre.column { mre.text { "right", size = 14 } },
            }
        end)
        "#,
    )
    .unwrap();

    assert_eq!(rt.tree(), r#"(row (text "left") (column (text "right")))"#);
}

#[test]
fn stats_expose_a_zero_cost_idle_frame() {
    let rt = Runtime::new().unwrap();
    rt.exec(
        r#"
        mre.mount(function() return mre.text "static" end)
        "#,
    )
    .unwrap();

    let stats = rt.stats();
    assert_eq!(stats.dirty, 0);
    assert_eq!(stats.last_flush_runs, 1, "the initial mount runs once");
    assert_eq!(stats.signals, 0);

    rt.flush().unwrap();
    assert_eq!(rt.stats().last_flush_runs, 0);
}
