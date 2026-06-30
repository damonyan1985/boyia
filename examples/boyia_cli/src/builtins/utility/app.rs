//! App builtin: process/app lifecycle helpers.

#![allow(dead_code)]

use builtin_macro::boyia_class;

struct AppBuiltins;

#[boyia_class(name = "App", registrar = builtin_app_class)]
impl AppBuiltins {
    /// Stop Boyia runner threads (runtime loop + thread pool).
    #[boyia_sync_builtin(method = "exit")]
    fn app_exit(ctx: crate::runner::builtin_ctx::BuiltinCtx) -> bool {
        ctx.stop_runner()
    }
}

