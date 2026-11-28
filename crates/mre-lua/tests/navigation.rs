//! Navigation is pure Lua composition, so its behaviour is tested at the
//! scripting layer: stack operations, codec round-trips, tabs and hot reload.

use mre_lua::Runtime;

const NAV: &str = include_str!("../../../framework/nav.lua");

const SETUP: &str = r#"
    mre.load('framework/nav')

    local function Home(ctx)
        return mre.text { 'home' }
    end

    local function Detail(ctx)
        return mre.text { 'detail ' .. tostring(ctx.params.id) }
    end

    nav = mre.nav {
        routes = { home = Home, detail = Detail },
        initial = 'home',
        transition_ms = 0,
        persist = 'test.nav',
    }
"#;

fn runtime() -> Runtime {
    let runtime = Runtime::with_loader(|name| (name == "framework/nav").then(|| NAV.to_string()))
        .expect("runtime with the navigation module");
    runtime.exec(SETUP).expect("define routes and navigator");
    runtime
}

#[test]
fn push_and_back_walk_the_stack() {
    let runtime = runtime();
    assert_eq!(runtime.eval::<String>("nav:top().name").unwrap(), "home");

    runtime.exec("nav:push('detail', { id = 7 })").unwrap();
    runtime.flush().unwrap();
    assert_eq!(runtime.eval::<String>("nav:top().name").unwrap(), "detail");
    assert_eq!(runtime.eval::<i64>("nav:top().params.id").unwrap(), 7);

    assert!(runtime.eval::<bool>("nav:back()").unwrap());
    runtime.flush().unwrap();
    assert_eq!(runtime.eval::<String>("nav:top().name").unwrap(), "home");

    // The root screen is never popped out from under the app.
    assert!(!runtime.eval::<bool>("nav:back()").unwrap());
}

#[test]
fn state_survives_a_hot_reload() {
    let runtime = runtime();
    runtime
        .exec("nav:push('detail', { id = 9, label = 'a;b:S3:xyz', nested = { 1, 2, k = 'v' } })")
        .unwrap();
    runtime.flush().unwrap();

    runtime.reload(SETUP).expect("reload re-executes the entry");
    runtime.exec("mre.__nav = nav").unwrap();
    assert_eq!(runtime.eval::<String>("nav:top().name").unwrap(), "detail");
    assert_eq!(runtime.eval::<i64>("nav:top().params.id").unwrap(), 9);
    assert_eq!(
        runtime.eval::<String>("nav:top().params.label").unwrap(),
        "a;b:S3:xyz",
        "length-prefixed strings must round-trip exactly"
    );
    assert_eq!(
        runtime.eval::<i64>("nav:top().params.nested[2]").unwrap(),
        2
    );
    assert_eq!(
        runtime.eval::<String>("nav:top().params.nested.k").unwrap(),
        "v"
    );
}

#[test]
fn modals_are_dismissed_before_the_stack() {
    let runtime = runtime();
    runtime.exec("nav:push('detail', { id = 3 })").unwrap();
    runtime
        .exec("nav:open_modal('detail', { id = 99 })")
        .unwrap();
    runtime.flush().unwrap();

    assert!(runtime.eval::<bool>("nav:back()").unwrap());
    runtime.flush().unwrap();
    assert_eq!(runtime.eval::<i64>("#nav:state().modals").unwrap(), 0);
    assert_eq!(runtime.eval::<String>("nav:top().name").unwrap(), "detail");
    assert_eq!(runtime.eval::<i64>("nav:top().params.id").unwrap(), 3);
}

#[test]
fn tabs_keep_independent_stacks() {
    let runtime =
        Runtime::with_loader(|name| (name == "framework/nav").then(|| NAV.to_string())).unwrap();
    runtime
        .exec(
            r#"
            mre.load('framework/nav')
            local function Home() return mre.text { 'home' } end
            local function Detail(ctx) return mre.text { tostring(ctx.params.id) } end
            nav = mre.nav {
                routes = { home = Home, detail = Detail },
                tabs = { 'chats', 'settings' },
                initial = 'home',
                transition_ms = 0,
                persist = 'test.tabs',
            }
            "#,
        )
        .unwrap();

    runtime.exec("nav:push('detail', { id = 11 })").unwrap();
    assert!(runtime.eval::<bool>("nav:set_tab('settings')").unwrap());
    assert_eq!(runtime.eval::<String>("nav:top().name").unwrap(), "home");
    assert!(runtime.eval::<bool>("nav:set_tab('chats')").unwrap());
    assert_eq!(runtime.eval::<i64>("nav:top().params.id").unwrap(), 11);
}
