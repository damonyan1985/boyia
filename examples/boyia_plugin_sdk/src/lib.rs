//! Shared infrastructure for boyia_cli and dylib plugin builtins.
//!
//! Proc macros are re-exported from the sibling `macro/` crate (`boyia_builtin_macro`).

pub mod embedder;
pub mod host;
pub mod layout;
pub mod plugin_export;
pub mod registrar;
pub mod runner;

#[cfg(feature = "build-install")]
pub mod build_install;

pub use embedder::{BuiltinAsyncHost, BuiltinCtx, SdkEmbedder};
pub use host::set_host_ctx_getter;
pub use layout::{default_plugin_dirs, ENV_ENABLE, ENV_PLUGIN_DIRS, ENV_STRICT, HOST_PLUGINS_SUBDIR};
pub use registrar::BuiltinRegistrar;

pub use boyia_builtin_macro::{boyia_class, boyia_native_object};

/// Re-exported for macro expansions in plugin crates (avoid direct `boyia_vm` / `boyia_gc` deps).
pub use boyia_gc;
pub use boyia_vm;

pub use runner::builtin_async::{
    attach_method, register_async_builtin_class, AsyncBuiltinResult, CallSite, ScriptCallback,
};
pub use runner::builtin_sync::{BoyiaScalar, SyncCallSite};
pub use runner::builtin_vec::NestedVec;

#[macro_export]
macro_rules! some_or_end {
    ($e:expr) => {
        match $e {
            Some(value) => value,
            None => return boyia_vm::OpHandleResult::kOpResultEnd,
        }
    };
}

#[macro_export]
macro_rules! define_async_native {
    ($native:ident, $min:expr, $handler:ident) => {
        unsafe fn $native(vm: &mut boyia_vm::BoyiaVM) -> boyia_vm::OpHandleResult {
            $crate::runner::builtin_async::async_dispatch(vm, $min, $handler)
        }
    };
}

#[macro_export]
macro_rules! define_sync_native {
    ($native:ident, $min:expr, $handler:ident) => {
        unsafe fn $native(vm: &mut boyia_vm::BoyiaVM) -> boyia_vm::OpHandleResult {
            $crate::runner::builtin_sync::sync_dispatch(vm, $min, $handler)
        }
    };
}

/// Optional: own `build.rs` that only invokes the install hook (prefer SDK shared script instead).
///
/// ```ignore
/// boyia_plugin_sdk::boyia_plugin_install_build!();
/// ```
///
/// Recommended: no local `build.rs` — in plugin `Cargo.toml` set
/// `build = "../boyia_plugin_sdk/build/plugin_install.rs"` and `[build-dependencies]` with feature `build-install`.
#[macro_export]
macro_rules! boyia_plugin_install_build {
    () => {
        fn main() {
            $crate::build_install::install_beside_host_exe();
        }
    };
}
