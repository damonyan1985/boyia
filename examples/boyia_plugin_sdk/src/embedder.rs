//! Async scheduling trait (implemented in boyia_cli) and embedder stored on BoyiaRuntime.

use boyia_runtime::{boyia_runtime_from_vm, BoyiaRuntime};
use boyia_vm::BoyiaVM;
use std::sync::Arc;

/// Worker pool + Boyia task thread; implemented by `boyia_cli` only.
pub trait BuiltinAsyncHost: Send + Sync {
    fn post_worker(&self, job: Box<dyn FnOnce() + Send>) -> bool;
    fn post_runtime(&self, job: Box<dyn FnOnce(&mut BoyiaRuntime) + Send>) -> bool;
    fn stop_runner(&self) -> bool;
}

/// Handle passed into async builtin handlers (`spawn`, persistent callbacks, etc.).
#[derive(Clone)]
pub struct BuiltinCtx {
    pub(crate) host: Arc<dyn BuiltinAsyncHost>,
}

impl BuiltinCtx {
    pub fn new(host: Arc<dyn BuiltinAsyncHost>) -> Self {
        Self { host }
    }

    pub fn post_runtime_task<F>(&self, task: F) -> bool
    where
        F: FnOnce(&mut BoyiaRuntime) + Send + 'static,
    {
        self.host.post_runtime(Box::new(task))
    }

    /// Stop thread-pool workers and request runtime run-loop stop (Host must implement via trait).
    pub fn stop_runner(&self) -> bool {
        self.host.stop_runner()
    }

    pub fn from_vm(vm: &mut BoyiaVM) -> Option<Self> {
        if let Some(ctx) = crate::host::host_ctx_from_vm(vm) {
            return Some(ctx);
        }
        unsafe {
            boyia_runtime_from_vm(vm)?
                .embedder::<SdkEmbedder>()
                .map(|e| e.builtin_ctx.clone())
        }
    }
}

/// Stored on [BoyiaRuntime] via embedder during CLI init.
#[derive(Clone)]
pub struct SdkEmbedder {
    pub builtin_ctx: BuiltinCtx,
}
