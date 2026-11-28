//! `mre-luac <file.lua>...` compiles every Lua file in order.
//!
//! It exists for `mre test`: an app file is checked in the same environment it
//! runs in, so a typo is reported before a device build is attempted.

use mlua::Table;
use mre_lua::Runtime;

fn main() {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: mre-luac <file.lua>...");
        std::process::exit(2);
    }
    let runtime = match Runtime::new() {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("lua runtime: {error}");
            std::process::exit(1);
        }
    };
    // `mre.load(name)` calls every module with the `mre` table as `...`.
    // Pass the same argument here, so a framework module using `local mre = ...`
    // is checked in the environment it actually runs in.
    let mre: Table = match runtime.lua().globals().get("mre") {
        Ok(table) => table,
        Err(error) => {
            eprintln!("lua runtime has no mre table: {error}");
            std::process::exit(1);
        }
    };
    for path in files {
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("{path}: {error}");
                std::process::exit(1);
            }
        };
        let result: mlua::Result<()> = runtime
            .lua()
            .load(&source)
            .set_name(path.clone())
            .call(&mre);
        if let Err(error) = result {
            eprintln!("{path}: {error}");
            std::process::exit(1);
        }
    }
}
