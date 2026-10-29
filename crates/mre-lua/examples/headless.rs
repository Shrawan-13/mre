//! Runs a Lua app headlessly and prints its vnode tree.
//!
//! ```text
//! cargo run -p mre-lua --example headless -- examples/counter.lua
//! ```
//!
//! With `--interact` it also simulates two clicks, showing that a state change
//! re-runs exactly one computation and leaves the graph idle afterwards.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: headless <app.lua> [--interact]");
        return ExitCode::from(2);
    };
    let interact = args.any(|a| a == "--interact");

    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("cannot read {path}: {err}");
            return ExitCode::from(2);
        }
    };

    let runtime = match mre_lua::Runtime::new() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("failed to start the Lua runtime: {err}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(err) = runtime.exec(&source) {
        eprintln!("{path}: {err}");
        return ExitCode::FAILURE;
    }

    println!("tree:\n  {}", runtime.tree());
    println!("stats: {:?}", runtime.stats());

    if interact {
        let stats_before = runtime.stats();
        if let Err(err) = runtime.exec("mre.__increment(); mre.__increment()") {
            eprintln!("interaction failed: {err}");
            return ExitCode::FAILURE;
        }
        if let Err(err) = runtime.flush() {
            eprintln!("flush failed: {err}");
            return ExitCode::FAILURE;
        }

        let stats_after = runtime.stats();
        println!("\nafter two clicks:");
        println!("  tree:  {}", runtime.tree());
        println!(
            "  reruns: {} (the view, the memo and its one observer; writes were coalesced)",
            stats_after.total_runs - stats_before.total_runs
        );

        if let Err(err) = runtime.flush() {
            eprintln!("flush failed: {err}");
            return ExitCode::FAILURE;
        }
        let idle = runtime.stats();
        println!(
            "  idle flush ran {} computations (expected 0)",
            idle.total_runs - stats_after.total_runs
        );
    }

    ExitCode::SUCCESS
}
