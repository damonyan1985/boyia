//! Example: use BoyiaRuntime to compile and run a Boyia program from `script/main.boyia`
//! (under this crate directory, resolved via `CARGO_MANIFEST_DIR`).
//! Prints results via `BY_Log` and exercises async/File/Https builtins.
//!
//! If the program hangs or crashes, see CRASH_ANALYSIS.md. You can set
//! env BOYIA_INIT_MINIMAL=1 to skip builtin classes (faster init, fewer deps)
//! and narrow down whether the crash is in init.

mod builtins;
mod runner;

use runner::BoyiaRunner;
fn main() {
    println!("Boyia CLI: compile and run script\n");

    let registrars = if std::env::var("BOYIA_INIT_MINIMAL").ok().as_deref() == Some("1") {
        &[][..]
    } else {
        builtins::DEFAULT_BUILTINS
    };

    println!("[1] Creating runtime...");
    let runner = BoyiaRunner::create(registrars);
    if !runner.is_ready() {
        eprintln!("Error: VM init returned null");
        return;
    }
    println!("[3] VM ready.");

    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("script")
        .join("main.boyia");
    let script = std::fs::read_to_string(&script_path).unwrap_or_else(|e| {
        panic!("failed to read {}: {e}", script_path.display());
    });

    println!("[4] Compiling script...");
    runner
        .compile(&script, Some(&script_path))
        .expect("failed to compile script on task thread");
    println!("[6] Running script...");

    println!("\nDone.");
    // When main returns, runner is dropped -> Drop stops task thread and joins it -> "BoyiaRunner exit!!!"
}
