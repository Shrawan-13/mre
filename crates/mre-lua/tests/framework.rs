//! The framework overlay is loaded at runtime, so app authors can restyle
//! without rebuilding the host binary.

use mre_lua::Runtime;

const STYLE: &str = include_str!("../../../framework/init.lua");

fn runtime() -> Runtime {
    Runtime::with_loader(|name| (name == "framework/init").then(|| STYLE.to_string()))
        .expect("runtime with a loaded style module")
}

#[test]
fn a_runtime_module_is_loaded_at_startup() {
    let runtime = runtime();
    assert!(
        runtime
            .eval::<bool>("type(mre.__apply_style) == 'function'")
            .unwrap()
    );
}

#[test]
fn a_style_sheet_reaches_every_node() {
    let runtime = runtime();
    runtime
        .exec(
            "mre.tokens { accent = '#123456', radius = 8 }\n\
             mre.sheet {\n\
               ['button'] = { background = 'var(accent)', radius = 'var(radius)' },\n\
               ['.danger'] = { color = '#FFFFFF' },\n\
             }\n\
             b = mre.button { 'Save', class = 'danger' }",
        )
        .expect("define styles and a button");
    assert_eq!(runtime.eval::<String>("b.background").unwrap(), "#123456");
    assert_eq!(runtime.eval::<f64>("b.radius").unwrap(), 8.0);
    assert_eq!(runtime.eval::<String>("b.color").unwrap(), "#FFFFFF");
}

#[test]
fn explicit_props_beat_the_style_sheet() {
    let runtime = runtime();
    runtime
        .exec(
            "mre.tokens { accent = '#123456' }\n\
             mre.sheet { ['button'] = { background = 'var(accent)' } }\n\
             b = mre.button { 'Save', background = '#000000' }",
        )
        .expect("define styles");
    assert_eq!(runtime.eval::<String>("b.background").unwrap(), "#000000");
}
