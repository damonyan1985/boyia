//! App builtin: process/app lifecycle helpers.

#![allow(dead_code)]

use boyia_plugin_sdk::boyia_class;

struct AppBuiltins;

#[boyia_class(name = "App", registrar = builtin_app_class)]
impl AppBuiltins {
    /// Stop Boyia runner threads (runtime loop + thread pool).
    #[boyia_sync_builtin(method = "exit")]
    fn app_exit(ctx: boyia_plugin_sdk::BuiltinCtx) -> bool {
        ctx.stop_runner()
    }
}

