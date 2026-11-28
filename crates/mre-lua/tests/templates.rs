use mre_lua::Runtime;

/// `mre new` embeds this file. A syntax error there would break every new
/// project and every dev-client reload, so compile it in CI.
#[test]
fn generated_starter_app_compiles() {
    let source = include_str!("../../../cli/internal/templates/app/src/main.lua");
    let runtime = Runtime::new().expect("runtime");
    runtime
        .exec(source)
        .expect("starter app must compile and mount");
}
