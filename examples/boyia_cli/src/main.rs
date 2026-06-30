//! Boyia CLI: compile and run a `.boyia` script from the command line or `.boyia_rc`.

mod builtins;
mod cli;
mod runner;

use cli::{parse_args, print_usage, resolve_entry_script};
use runner::BoyiaRunner;
use std::time::Duration;

fn main() {
    let mut env_args: Vec<String> = std::env::args().skip(1).collect();
    let program = std::env::args()
        .next()
        .unwrap_or_else(|| "boyia_cli".into());

    let script_arg = match parse_args(std::mem::take(&mut env_args)) {
        Ok(None) => {
            print_usage(&program);
            return;
        }
        Ok(Some(path)) if path.is_empty() => None,
        Ok(Some(path)) => Some(path),
        Err(err) => {
            eprintln!("Error: {err}\n");
            print_usage(&program);
            std::process::exit(1);
        }
    };

    let script_path = match resolve_entry_script(script_arg.as_deref()) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("Error: {err}\n");
            print_usage(&program);
            std::process::exit(1);
        }
    };

    println!("Boyia CLI: {}", script_path.display());

    let registrars = if std::env::var("BOYIA_INIT_MINIMAL").ok().as_deref() == Some("1") {
        &[][..]
    } else {
        builtins::DEFAULT_BUILTINS
    };

    let runner = BoyiaRunner::create(registrars);
    if !runner.is_ready() {
        eprintln!("Error: VM init failed");
        std::process::exit(1);
    }

    if let Err(e) = runner.compile_file(&script_path) {
        eprintln!("Error: compile failed: {e:?}");
        std::process::exit(1);
    }

    if let Err(e) = runner.run_exe_file() {
        eprintln!("Error: run failed: {e:?}");
        std::process::exit(1);
    }

    //std::thread::sleep(Duration::from_millis(20000));
}
