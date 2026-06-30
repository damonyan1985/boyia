//! Runner wrapper that binds BoyiaRuntime to a dedicated TaskThread.

#![allow(dead_code)]

use super::builtin_ctx::{BuiltinCtx, CliEmbedder};
use super::run_loop::RunLoopError;
use super::task_thread::TaskThread;
use super::thread_pool::ThreadPool;
use super::BuiltinRegistrar;
use boyia_runtime::BoyiaRuntime;
use std::sync::{mpsc, Arc};

const DEFAULT_HTTPS_THREAD_COUNT: usize = 4;

pub struct BoyiaRunner {
    boyia_thread: Option<TaskThread<Box<BoyiaRuntime>>>,
    thread_pool: Option<Arc<ThreadPool>>,
    ready: bool,
}

impl BoyiaRunner {
    /// Create a runner and register `builtins` on the Boyia task thread after VM init.
    pub fn create(builtins: &[BuiltinRegistrar]) -> Box<Self> {
        let builtins: Vec<BuiltinRegistrar> = builtins.to_vec();
        Self::create_with_thread_pool(builtins, DEFAULT_HTTPS_THREAD_COUNT)
    }

    pub fn create_with_thread_pool(
        builtins: Vec<BuiltinRegistrar>,
        https_thread_count: usize,
    ) -> Box<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();

        let task_thread = TaskThread::start_with_init("boyia-runner", move |_| {
            let runtime = BoyiaRuntime::create();
            let ready = !runtime.vm().is_null();
            let _ = ready_tx.send(ready);
            runtime
        });

        let ready = ready_rx.recv().unwrap_or(false);
        let thread_pool = Arc::new(ThreadPool::new(https_thread_count));

        let runner = Self {
            boyia_thread: Some(task_thread),
            thread_pool: Some(thread_pool),
            ready,
        };
        let runner_box = Box::new(runner);

        let builtin_ctx = BuiltinCtx::new(
            runner_box.boyia_thread.as_ref().unwrap().handle(),
            Arc::downgrade(runner_box.thread_pool.as_ref().unwrap()),
        );

        let (init_tx, init_rx) = mpsc::channel();
        let embedder = CliEmbedder {
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

    /// Compile the entry script directly from its path (reads the file, resolves `require`
    /// dependencies via post-order DFS). Replaces reading the source and calling `compile`.
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

    /// Gracefully stop thread-pool workers and the Boyia task thread.
    ///
    /// This method is idempotent-ish: repeated calls may return `RunLoopError::Stopped`
    /// from already-stopped run loops.
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
