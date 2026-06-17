//! Config builtin: native heap state (`nativePtr` + `Box`) with vtable GC.

#![allow(dead_code)]

use builtin_macro::{boyia_class, boyia_native_object};

#[boyia_native_object]
pub struct ConfigBuiltins {
    #[boyia_field_default = "false"]
    debug: bool,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(method = "getDebug")]
    fn get_debug(&self) -> bool {
        self.debug
    }

    #[boyia_sync_builtin(method = "setDebug")]
    fn set_debug(&mut self, value: bool) {
        self.debug = value;
    }

    #[boyia_sync_builtin(method = "getTimeout")]
    fn get_timeout(&self) -> u64 {
        self.timeout_ms * 2
    }

    #[boyia_sync_builtin(method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
}
