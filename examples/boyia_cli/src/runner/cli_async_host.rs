//! CLI implementation of [boyia_plugin_sdk::BuiltinAsyncHost] using thread pool + task thread.

use boyia_plugin_sdk::BuiltinAsyncHost;
use boyia_runtime::BoyiaRuntime;
use super::run_loop::RunLoopHandle;
use super::thread_pool::ThreadPool;
use std::sync::Weak;

pub struct CliBuiltinAsyncHost {
    runtime_handle: RunLoopHandle<Box<BoyiaRuntime>>,
    thread_pool: Weak<ThreadPool>,
}

impl CliBuiltinAsyncHost {
    pub fn new(
        runtime_handle: RunLoopHandle<Box<BoyiaRuntime>>,
        thread_pool: Weak<ThreadPool>,
    ) -> Self {
        Self {
            runtime_handle,
            thread_pool,
        }
    }
}

impl BuiltinAsyncHost for CliBuiltinAsyncHost {
    fn post_worker(&self, job: Box<dyn FnOnce() + Send>) -> bool {
        let Some(pool) = self.thread_pool.upgrade() else {
            return false;
        };
        pool.post_task(move || job()).is_ok()
    }

    fn post_runtime(&self, job: Box<dyn FnOnce(&mut BoyiaRuntime) + Send>) -> bool {
        self.runtime_handle
            .post_task(move |runtime| job(runtime.as_mut()))
            .is_ok()
    }

    fn stop_runner(&self) -> bool {
        if let Some(thread_pool) = self.thread_pool.upgrade() {
            let _ = thread_pool.stop();
        }
        self.runtime_handle.stop().is_ok()
    }
}
