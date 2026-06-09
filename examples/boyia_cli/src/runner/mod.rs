//! Boyia task thread, thread pool, async builtin infrastructure, and [BoyiaRunner].

use boyia_vm::{BoyiaVM, LUintPtr};

/// Register one global builtin class on the VM (called on the Boyia task thread during init).
pub type BuiltinRegistrar = fn(&mut BoyiaVM, &mut dyn FnMut(&str) -> LUintPtr);

pub(crate) mod r#async;
mod run_loop;
mod runner;
mod task_thread;
mod thread_pool;

pub use runner::BoyiaRunner;