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

    let app = match mre_runtime::App::new(&source) {
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
