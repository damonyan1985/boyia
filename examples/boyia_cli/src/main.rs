//! Boyia CLI: compile and run a `.boyia` script from the command line or `.boyia_rc`.

mod builtins;
mod cli;
mod runner;

use builtins::DEFAULT_BUILTINS;
use cli::{parse_args, print_usage, resolve_entry_script};
use runner::{
    dylib_builtins_enabled, plugin_dirs_from_env, plugin_strict, BoyiaRunner, PluginRegistry,
};
use boyia_plugin_sdk::BuiltinRegistrar;

/// Script-relative paths in tests use the `boyia_cli` crate root (see `test/zip.boyia`).
fn set_cli_crate_working_dir() {
    if std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).is_err() {
        if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
            let _ = std::env::set_current_dir(manifest_dir);
        }
    }
}

fn main() {
    set_cli_crate_working_dir();

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

    let minimal = std::env::var("BOYIA_INIT_MINIMAL").ok().as_deref() == Some("1");
    let static_registrars: Vec<BuiltinRegistrar> = if minimal {
        vec![]
    } else {
        DEFAULT_BUILTINS.to_vec()
    };

    let runner = if dylib_builtins_enabled() && !minimal {
        match PluginRegistry::load_from_dirs(&plugin_dirs_from_env()) {
            Ok(plugins) => BoyiaRunner::create_with_plugins(static_registrars, plugins),
            Err(e) if plugin_strict() => {
                eprintln!("Error: plugin load failed: {e}");
                std::process::exit(1);
            }
            Err(e) => {
                eprintln!("Warning: plugin load failed: {e}");
                BoyiaRunner::create(&static_registrars)
            }
        }
    } else {
        BoyiaRunner::create(&static_registrars)
    };

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
}
