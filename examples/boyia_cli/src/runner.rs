//! Runner wrapper that binds BoyiaRuntime to a dedicated TaskThread.

#![allow(dead_code)]

use crate::builtins::r#async::{AsyncCtx, CliEmbedder};
use crate::builtins::file::builtin_file_class;
use crate::builtins::https::builtin_https_class;
use crate::builtins::zip::builtin_zip_class;
use crate::run_loop::RunLoopError;
use crate::task_thread::TaskThread;
use crate::thread_pool::ThreadPool;
use boyia_runtime::BoyiaRuntime;
use std::sync::{mpsc, Arc};

const DEFAULT_HTTPS_THREAD_COUNT: usize = 4;

pub struct BoyiaRunner {
    boyia_thread: Option<TaskThread<Box<BoyiaRuntime>>>,
    thread_pool: Option<Arc<ThreadPool>>,
    ready: bool,
}

impl BoyiaRunner {
    pub fn create() -> Box<Self> {
        let (ready_tx, ready_rx) = mpsc::channel();

        let task_thread = TaskThread::start_with_init("boyia-runner", move |_| {
            let runtime = BoyiaRuntime::create();
            let ready = !runtime.vm().is_null();
            let _ = ready_tx.send(ready);
            runtime
        });

        let ready = ready_rx.recv().unwrap_or(false);
        let thread_pool = Arc::new(ThreadPool::new(DEFAULT_HTTPS_THREAD_COUNT));

        let runner = Self {
            boyia_thread: Some(task_thread),
            thread_pool: Some(thread_pool),
            ready,
        };
        let runner_box = Box::new(runner);

        let async_ctx = AsyncCtx::new(
            runner_box.boyia_thread.as_ref().unwrap().handle(),
            Arc::downgrade(runner_box.thread_pool.as_ref().unwrap()),
        );

        let (init_tx, init_rx) = mpsc::channel();
        let embedder = CliEmbedder {
            async_ctx: async_ctx.clone(),
        };
        let _ = runner_box.boyia_thread.as_ref().unwrap().post_task(move |runtime| {
            let runtime = runtime.as_mut();
            runtime.set_embedder(embedder);
            let _ = runtime.with_vm_and_id_creator(|vm, id_creator| {
                let mut gen_id = |s: &str| id_creator.gen_ident_by_str(s);
                builtin_https_class(vm, &mut gen_id);
                builtin_file_class(vm, &mut gen_id);
                builtin_zip_class(vm, &mut gen_id);
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

    pub fn compile(
        &self,
        script: &str,
        entry_script: Option<&std::path::Path>,
    ) -> Result<(), RunLoopError> {
        let script = script.to_string();
        let entry = entry_script.map(|p| p.to_path_buf());
        let (done_tx, done_rx) = mpsc::channel();
        self.post_runtime_task(move |runtime| {
            if let Some(ref p) = entry {
                runtime.set_entry_script_path(&p.to_string_lossy());
            }
            runtime.compile(&script);
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
}

impl Drop for BoyiaRunner {
    fn drop(&mut self) {
        std::thread::sleep(std::time::Duration::from_secs(10));

        if let Some(thread_pool) = self.thread_pool.take() {
            let _ = thread_pool.stop();
            if let Ok(thread_pool) = Arc::try_unwrap(thread_pool) {
                let _ = thread_pool.join();
            }
        }

        if let Some(ref boyia_thread) = self.boyia_thread {
            let _ = boyia_thread.stop();
        }
        if let Some(boyia_thread) = self.boyia_thread.take() {
            let _ = boyia_thread.join();
        }

        println!("BoyiaRunner exit!!!");
    }
}
