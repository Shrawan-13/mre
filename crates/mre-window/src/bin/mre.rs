//! `mre <app.lua>`: run a Lua application in a window.

fn main() {
    let path = match std::env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: mre <app.lua>");
            std::process::exit(2);
        }
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            std::process::exit(1);
        }
    };

    // Framework Lua is loaded at runtime: edit a file under `framework/` and
    // rerun the app — no Rust rebuild is needed for styles or widgets.
    let framework_dir = std::env::var("MRE_FRAMEWORK").unwrap_or_else(|_| "framework".into());
    let app = match mre_runtime::App::with_lua_loader(&source, move |name| {
        let relative = name.strip_prefix("framework/").unwrap_or(name);
        let path = std::path::Path::new(&framework_dir).join(format!("{relative}.lua"));
        std::fs::read_to_string(path).ok()
    }) {
        Ok(app) => app,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };

    if let Err(error) = mre_window::run(app, "MRE") {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
