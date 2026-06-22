//! Boyia task thread, thread pool, async builtin infrastructure, and [BoyiaRunner].

use boyia_vm::{BoyiaVM, LUintPtr};

/// Register one global builtin class on the VM (called on the Boyia task thread during init).
pub type BuiltinRegistrar = fn(&mut BoyiaVM, &mut dyn FnMut(&str) -> LUintPtr);

#[path = "macro/builtin_json.rs"]
pub(crate) mod builtin_json;

#[path = "macro/builtin_vec.rs"]
pub(crate) mod builtin_vec;

#[path = "macro/builtin_async.rs"]
pub(crate) mod builtin_async;

#[path = "macro/builtin_sync.rs"]
pub(crate) mod builtin_sync;
mod run_loop;
mod runner;
mod task_thread;
mod thread_pool;

pub use runner::BoyiaRunner;