//! Boyia task thread, thread pool, plugin loader, and [BoyiaRunner].

pub use boyia_plugin_sdk::BuiltinRegistrar;

mod cli_async_host;
mod plugin_loader;
mod run_loop;
mod runner;
mod task_thread;
mod thread_pool;

pub use plugin_loader::{dylib_builtins_enabled, plugin_dirs_from_env, plugin_strict, PluginRegistry};
pub use runner::BoyiaRunner;
