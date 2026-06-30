//! Shared builtin execution context for async/sync builtins.

use crate::runner::run_loop::RunLoopHandle;
use crate::runner::thread_pool::ThreadPool;
use boyia_runtime::{boyia_runtime_from_vm, BoyiaRuntime};
use boyia_vm::BoyiaVM;
use std::sync::Weak;

/// Stored on [BoyiaRuntime] via embedder during CLI init.
#[derive(Clone)]
pub struct CliEmbedder {
    pub builtin_ctx: BuiltinCtx,
}

/// Safe handle for posting work to the Boyia task thread and worker pool.
#[derive(Clone)]
pub struct BuiltinCtx {
    pub(crate) runtime_handle: RunLoopHandle<Box<BoyiaRuntime>>,
    pub(crate) thread_pool: Weak<ThreadPool>,
}

impl BuiltinCtx {
    pub fn new(runtime_handle: RunLoopHandle<Box<BoyiaRuntime>>, thread_pool: Weak<ThreadPool>) -> Self {
        Self {
            runtime_handle,
            thread_pool,
        }
    }

    pub fn post_runtime_task<F>(&self, task: F) -> bool
    where
        F: FnOnce(&mut BoyiaRuntime) + Send + 'static,
    {
        self.runtime_handle
            .post_task(move |runtime| task(runtime.as_mut()))
            .is_ok()
    }

    /// Stop thread-pool workers and request runtime run-loop stop.
    pub fn stop_runner(&self) -> bool {
        if let Some(thread_pool) = self.thread_pool.upgrade() {
            let _ = thread_pool.stop();
        }
        self.runtime_handle.stop().is_ok()
    }
}

pub fn builtin_ctx_from_vm(vm: &mut BoyiaVM) -> Option<BuiltinCtx> {
    unsafe {
        boyia_runtime_from_vm(vm)?
            .embedder::<CliEmbedder>()
            .map(|e| e.builtin_ctx.clone())
    }
}
