//! Host-injected context lookup for dylib plugins (avoids cross-DLL TypeId downcast).

use boyia_vm::BoyiaVM;
use std::sync::Mutex;

use crate::embedder::BuiltinCtx;

static HOST_CTX_GETTER: Mutex<Option<fn(&mut BoyiaVM) -> Option<BuiltinCtx>>> = Mutex::new(None);

/// Called by boyia_cli on the Boyia task thread before registering builtins.
pub fn set_host_ctx_getter(getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
    *HOST_CTX_GETTER.lock().unwrap_or_else(|e| e.into_inner()) = Some(getter);
}

/// Called once per loaded plugin dylib (each DLL has its own copy of this static).
#[no_mangle]
pub unsafe extern "C" fn boyia_plugin_bind_host_ctx(getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
    *HOST_CTX_GETTER.lock().unwrap_or_else(|e| e.into_inner()) = Some(getter);
}

pub(crate) fn host_ctx_from_vm(vm: &mut BoyiaVM) -> Option<BuiltinCtx> {
    let guard = HOST_CTX_GETTER.lock().unwrap_or_else(|e| e.into_inner());
    let getter = *guard.as_ref()?;
    getter(vm)
}
