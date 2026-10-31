//! Shared Cargo build script for cdylib plugins. Reference from plugin `Cargo.toml`:
//!
//! ```toml
//! build = "../boyia_plugin_sdk/build/plugin_install.rs"
//!
//! [build-dependencies]
//! boyia_plugin_sdk = { path = "../boyia_plugin_sdk", features = ["build-install"] }
//! ```

fn main() {
    boyia_plugin_sdk::build_install::install_beside_host_exe();
}
