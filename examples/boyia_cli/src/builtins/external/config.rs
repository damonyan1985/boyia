//! Config builtin: class fields (`debug`, `timeoutMs`) mapped to Boyia class properties via `#[boyia_fields]`.

#![allow(dead_code)]

use builtin_macro::{boyia_class, boyia_fields};

#[boyia_fields]
pub struct ConfigBuiltins {
    #[boyia_default = "false"]
    debug: bool,
    #[boyia_default = "30000"]
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class, fields)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(native = config_get_debug_native, method = "getDebug")]
    fn get_debug(&self) -> bool {
        self.debug
    }

    #[boyia_sync_builtin(native = config_set_debug_native, method = "setDebug")]
    fn set_debug(&mut self, value: bool) {
        self.debug = value;
    }

    #[boyia_sync_builtin(native = config_get_timeout_native, method = "getTimeout")]
    fn get_timeout(&self) -> u64 {
        let timeout = self.timeout_ms * 2;
        timeout
    }

    #[boyia_sync_builtin(native = config_set_timeout_native, method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
}
