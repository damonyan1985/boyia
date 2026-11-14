//! Runner wrapper that binds BoyiaRuntime to a dedicated TaskThread.

#![allow(dead_code)]

use super::cli_async_host::CliBuiltinAsyncHost;
use super::run_loop::RunLoopError;
use super::task_thread::TaskThread;
use super::thread_pool::ThreadPool;
use boyia_plugin_sdk::{
    set_host_ctx_getter, BuiltinCtx, BuiltinRegistrar, PluginRegistry, SdkEmbedder,
};
use boyia_runtime::{boyia_runtime_from_vm, BoyiaRuntime};
use boyia_vm::BoyiaVM;
use std::sync::{mpsc, Arc};

const DEFAULT_POOL_THREAD_COUNT: usize = 4;

pub struct BoyiaRunner {
    boyia_thread: Option<TaskThread<Box<BoyiaRuntime>>>,
    thread_pool: Option<Arc<ThreadPool>>,
    ready: bool,
    _plugin_registry: Option<PluginRegistry>,
}

impl BoyiaRunner {
    /// Create a runner and register `builtins` on the Boyia task thread after VM init.
    pub fn create(builtins: &[BuiltinRegistrar]) -> Box<Self> {
        Self::create_impl(builtins.to_vec(), None)
    }

    /// Static builtins plus registrars from loaded dylib plugins.
    pub fn create_with_plugins(
        static_builtins: Vec<BuiltinRegistrar>,
        plugins: PluginRegistry,
    ) -> Box<Self> {
        let mut all = static_builtins;
        all.extend(plugins.registrars().iter().copied());
        Self::create_impl(all, Some(plugins))
    }

    pub fn create_with_thread_pool(
        builtins: Vec<BuiltinRegistrar>,
        pool_thread_count: usize,
    ) -> Box<Self> {
        Self::create_impl_with_pool(builtins, None, pool_thread_count)
    }

    fn create_impl(builtins: Vec<BuiltinRegistrar>, plugins: Option<PluginRegistry>) -> Box<Self> {
        Self::create_impl_with_pool(builtins, plugins, DEFAULT_POOL_THREAD_COUNT)
    }

    fn create_impl_with_pool(
        builtins: Vec<BuiltinRegistrar>,
        plugins: Option<PluginRegistry>,
        pool_thread_count: usize,
    ) -> Box<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();

        let task_thread = TaskThread::start_with_init("boyia-runner", move |_| {
            let runtime = BoyiaRuntime::create();
            let ready = !runtime.vm().is_null();
            let _ = ready_tx.send(ready);
            runtime
        });

        let ready = ready_rx.recv().unwrap_or(false);
        let thread_pool = Arc::new(ThreadPool::new(pool_thread_count));

        let runner = Self {
            boyia_thread: Some(task_thread),
            thread_pool: Some(thread_pool.clone()),
            ready,
            _plugin_registry: plugins,
        };
        let runner_box = Box::new(runner);

        let async_host = Arc::new(CliBuiltinAsyncHost::new(
            runner_box.boyia_thread.as_ref().unwrap().handle(),
            Arc::downgrade(&thread_pool),
        ));
        let builtin_ctx = BuiltinCtx::new(async_host);

        set_host_ctx_getter(host_ctx_getter);

        if let Some(plugins) = runner_box._plugin_registry.as_ref() {
            plugins.bind_host_ctx(host_ctx_getter);
        }

        let (init_tx, init_rx) = mpsc::channel();
        let embedder = SdkEmbedder {
            builtin_ctx: builtin_ctx.clone(),
        };
        let _ = runner_box.boyia_thread.as_ref().unwrap().post_task(move |runtime| {
            let runtime = runtime.as_mut();
            runtime.set_embedder(embedder);
            let _ = runtime.with_vm_and_id_creator(|vm, id_creator| {
                let mut gen_id = |s: &str| id_creator.gen_ident_by_str(s);
                for register in &builtins {
                    register(vm, &mut gen_id);
                }
            });
            let _ = init_tx.send(());
        });
        let _ = init_rx.recv();

        runner_box
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    fn post_runtime_task<F>(&self, task: F) -> Result<(), RunLoopError>
    where
        F: FnOnce(&mut BoyiaRuntime) + Send + 'static,
    {
        self.boyia_thread
            .as_ref()
            .expect("task thread already taken")
            .post_task(move |runtime| task(runtime.as_mut()))
    }

    pub fn compile_file(&self, entry_script: &std::path::Path) -> Result<(), RunLoopError> {
        let path = entry_script.to_path_buf();
        let (done_tx, done_rx) = mpsc::channel();
        self.post_runtime_task(move |runtime| {
            let p = path.to_string_lossy().into_owned();
            runtime.set_entry_script_path(&p);
            runtime.compile_file(&p);
            let _ = done_tx.send(());
        })?;
        let _ = done_rx.recv();
        Ok(())
    }

    pub fn run_exe_file(&self) -> Result<(), RunLoopError> {
        let (done_tx, done_rx) = mpsc::channel();
        self.post_runtime_task(move |runtime| {
            runtime.run_exe_file();
            let _ = done_tx.send(());
        })?;
        let _ = done_rx.recv();
        Ok(())
    }

    pub fn consume_micro_task(&self) -> Result<(), RunLoopError> {
        let (done_tx, done_rx) = mpsc::channel();
        self.post_runtime_task(move |runtime| {
            runtime.consume_micro_task();
            let _ = done_tx.send(());
        })?;
        let _ = done_rx.recv();
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), RunLoopError> {
        let mut stop_err = None;

        if let Some(thread_pool) = self.thread_pool.as_ref() {
            if let Err(err) = thread_pool.stop() {
                stop_err = Some(err);
            }
        }

        if let Some(boyia_thread) = self.boyia_thread.as_ref() {
            if let Err(err) = boyia_thread.stop() {
                if stop_err.is_none() {
                    stop_err = Some(err);
                }
            }
        }

        self.ready = false;
        if let Some(err) = stop_err {
            return Err(err);
        }
        Ok(())
    }
}

fn host_ctx_getter(vm: &mut BoyiaVM) -> Option<BuiltinCtx> {
    unsafe {
        boyia_runtime_from_vm(vm)?
            .embedder::<SdkEmbedder>()
            .map(|e| e.builtin_ctx.clone())
    }
}

impl Drop for BoyiaRunner {
    fn drop(&mut self) {
        if let Some(boyia_thread) = self.boyia_thread.take() {
            let _ = boyia_thread.join();
        }

        if let Some(thread_pool) = self.thread_pool.take() {
            if let Ok(thread_pool) = Arc::try_unwrap(thread_pool) {
                let _ = thread_pool.join();
            }
        }
        println!("BoyiaRunner exit!!!");
    }
}
