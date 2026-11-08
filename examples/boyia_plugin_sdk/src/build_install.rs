//! Copy built cdylib into `target/{profile}/plugins/` (beside `boyia_cli.exe`). Used from plugin `build.rs`.
//! Enable feature `build-install` on a `[build-dependencies]` entry for `boyia_plugin_sdk`.

use std::path::PathBuf;

/// Install hook for plugin `build.rs` (link into `plugins/`, sync copy when artifact already exists).
pub fn install_beside_host_exe() {
    let profile = std::env::var("PROFILE").expect("PROFILE");
    let pkg = std::env::var("CARGO_PKG_NAME").expect("CARGO_PKG_NAME");
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let workspace = workspace_root(&manifest);
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace.join("target"));

    let file_name = cdylib_file_name(&pkg);
    let artifact = target_dir.join(&profile).join(&file_name);
    let plugins_dir = target_dir.join(&profile).join(crate::layout::HOST_PLUGINS_SUBDIR);
    let install = plugins_dir.join(&file_name);

    let _ = std::fs::create_dir_all(&plugins_dir);

    // Incremental builds: previous link output may already exist before this build.rs run.
    if artifact.exists() {
        match std::fs::copy(&artifact, &install) {
            Ok(_) => eprintln!("boyia_plugin_sdk: installed {}", install.display()),
            Err(e) => eprintln!("boyia_plugin_sdk: install copy failed: {e}"),
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    if artifact.exists() {
        println!("cargo:rerun-if-changed={}", artifact.display());
    }

    // Prefer linking directly into plugins/ (runs during link, after build.rs returns).
    let out = install.display().to_string();
    #[cfg(all(windows, target_env = "msvc"))]
    {
        println!("cargo:rustc-link-arg=/OUT:{out}");
    }
    #[cfg(not(all(windows, target_env = "msvc")))]
    {
        println!("cargo:rustc-link-arg=-o");
        println!("cargo:rustc-link-arg={out}");
    }
}

/// Walk up from the plugin manifest until a `Cargo.toml` containing `[workspace]` is found,
/// so plugins can live at any depth under `examples/`.
fn workspace_root(manifest: &PathBuf) -> PathBuf {
    let mut dir = manifest.clone();
    loop {
        let cargo = dir.join("Cargo.toml");
        if cargo.is_file() {
            if let Ok(text) = std::fs::read_to_string(&cargo) {
                if text.lines().any(|line| line.trim() == "[workspace]") {
                    return dir;
                }
            }
        }
        if !dir.pop() {
            return manifest.clone();
        }
    }
}

fn cdylib_file_name(crate_name: &str) -> String {
    if cfg!(windows) {
        format!("{crate_name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{crate_name}.dylib")
    } else {
        format!("lib{crate_name}.so")
    }
}
