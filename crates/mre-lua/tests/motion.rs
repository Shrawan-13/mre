//! Time-based animation helpers exposed to Lua.

use mre_lua::Runtime;

#[test]
fn writing_a_bound_signal_retargets_its_spring() {
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec("s = mre.signal(0); a = mre.animate(s)")
        .expect("create bound spring");
    assert_eq!(runtime.eval::<f64>("a.value()").unwrap(), 0.0);

    // The natural way to drive a bound spring: write its signal.
    runtime.exec("s(1)").expect("retarget through signal");
    assert!(
        runtime.animating(),
        "a signal write must wake the scheduler"
    );

    runtime.tick(0.016).expect("tick");
    let value = runtime.eval::<f64>("a.value()").unwrap();
    assert!(value > 0.0 && value < 1.0, "spring moved to {value}");
    assert!(runtime.eval::<f64>("s()").unwrap() > 0.0);
}

#[test]
fn a_tween_follows_its_easing_curve() {
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec("tween = mre.tween(0, 1, { duration = 100, easing = 'linear' })")
        .expect("create tween");
    assert_eq!(runtime.eval::<f64>("tween.value()").unwrap(), 0.0);

    runtime.tick(0.05).expect("tick");
    let half = runtime.eval::<f64>("tween.value()").unwrap();
    assert!((half - 0.5).abs() < 0.001, "half = {half}");

    runtime.tick(0.1).expect("tick");
    assert_eq!(runtime.eval::<f64>("tween.value()").unwrap(), 1.0);
    assert!(!runtime.animating());
}

#[test]
fn a_tween_can_be_retargeted_from_its_current_value() {
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec("tween = mre.tween(0, 10, { duration = 100, easing = 'linear' })")
        .unwrap();
    runtime.tick(0.05).unwrap();
    runtime.exec("tween.to(2)").unwrap();
    let value = runtime.eval::<f64>("tween.value()").unwrap();
    assert!((value - 5.0).abs() < 0.001, "retarget from {value}");
}

#[test]
fn a_timer_fires_once_and_stops_requesting_frames() {
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec("fired = 0; mre.after(100, function() fired = fired + 1 end)")
        .unwrap();
    assert!(runtime.animating());
    runtime.tick(0.05).unwrap();
    assert_eq!(runtime.eval::<i64>("fired").unwrap(), 0);
    runtime.tick(0.06).unwrap();
    assert_eq!(runtime.eval::<i64>("fired").unwrap(), 1);
    assert!(!runtime.animating());
}

#[test]
fn safe_area_is_exposed_to_lua() {
    let mut runtime = Runtime::new().expect("runtime");
    runtime.set_safe_area(24.0, 12.0, 16.0, 8.0);
    runtime.exec("s = mre.safe_area()").unwrap();
    assert_eq!(runtime.eval::<f64>("s.top").unwrap(), 24.0);
    assert_eq!(runtime.eval::<f64>("s.right").unwrap(), 12.0);
    assert_eq!(runtime.eval::<f64>("s.bottom").unwrap(), 16.0);
    assert_eq!(runtime.eval::<f64>("s.left").unwrap(), 8.0);
}
