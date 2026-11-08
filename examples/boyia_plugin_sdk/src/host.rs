//! Host-injected context lookup for dylib plugins (avoids cross-DLL TypeId downcast).

use boyia_vm::BoyiaVM;
use std::sync::OnceLock;

use crate::embedder::BuiltinCtx;

static HOST_CTX_GETTER: OnceLock<fn(&mut BoyiaVM) -> Option<BuiltinCtx>> = OnceLock::new();

/// Called by boyia_cli on the Boyia task thread before registering builtins.
///
/// Write-once: later calls are ignored. The getter is stateless, so re-initializing
/// with the same function pointer is a no-op.
pub fn set_host_ctx_getter(getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
    let _ = HOST_CTX_GETTER.set(getter);
}

/// Called once per loaded plugin dylib (each DLL has its own copy of this static).
#[no_mangle]
pub unsafe extern "C" fn boyia_plugin_bind_host_ctx(getter: fn(&mut BoyiaVM) -> Option<BuiltinCtx>) {
    let _ = HOST_CTX_GETTER.set(getter);
}

pub(crate) fn host_ctx_from_vm(vm: &mut BoyiaVM) -> Option<BuiltinCtx> {
    // No lock is held across the getter call (OnceLock reads are lock-free).
    let getter = *HOST_CTX_GETTER.get()?;
    getter(vm)
}
