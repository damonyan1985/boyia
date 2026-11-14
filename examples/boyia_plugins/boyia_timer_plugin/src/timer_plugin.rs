//! Timer plugin: `setTimeout` / `clearTimeout` / `now` on top of
//! [`crate::scheduler::TimerScheduler`].
//!
//! Both methods are **sync** builtins: they only enqueue / remove a timer on the scheduler
//! thread and return immediately (timer id / cancelled flag), so the VM thread is never
//! blocked. The script callback is captured with `callback = "owned"`, which keeps the
//! anonymous function alive across `pop_scene` and hands its ownership to this plugin.
//!
//! When the deadline fires, the scheduler thread posts a runtime task that invokes the
//! callback on the VM thread (`BuiltinCtx::post_runtime_task`). A cancelled timer releases
//! the callback instead of invoking it.

use boyia_plugin_sdk::boyia_class;
use boyia_plugin_sdk::boyia_vm::{vm_from_void, BoyiaValue};
use boyia_plugin_sdk::builtin::builtin_async::{
    invoke_script_callback, release_script_callback, ScriptCallback,
};
use boyia_plugin_sdk::embedder::BuiltinCtx;

use crate::scheduler::{TimerOutcome, TimerScheduler};

struct TimerBuiltins;

#[boyia_class(name = "TimerPlugin", registrar = builtin_timer_plugin_class)]
impl TimerBuiltins {
    /// `setTimeout(ms, callback)` — fires `callback` once after `ms` milliseconds.
    /// Returns the timer id, which can be passed to `clearTimeout`.
    #[boyia_sync_builtin(method = "setTimeout", callback = "owned")]
    fn set_timeout(ctx: BuiltinCtx, ms: usize, callback: ScriptCallback) -> usize {
        TimerScheduler::global().schedule(ms as u64, Box::new(move |outcome| {
            let _ = ctx.post_runtime_task(move |runtime| {
                let vm_ptr = runtime.vm();
                if vm_ptr.is_null() {
                    return;
                }
                let Some(vm) = (unsafe { vm_from_void(vm_ptr) }) else {
                    return;
                };
                match outcome {
                    TimerOutcome::Fired => {
                        // Zero-argument callback; invoke releases the captured closure.
                        let mut no_args: [BoyiaValue; 0] = [];
                        let _ = invoke_script_callback(vm, callback, &mut no_args);
                        runtime.consume_micro_task();
                    }
                    TimerOutcome::Cancelled => release_script_callback(vm, callback),
                }
            });
        }))
    }

    /// `clearTimeout(id)` — returns 1 when a pending timer was cancelled, 0 otherwise.
    #[boyia_sync_builtin(method = "clearTimeout")]
    fn clear_timeout(id: usize) -> bool {
        TimerScheduler::global().cancel(id)
    }

    /// `now()` — current wall-clock time in milliseconds since the Unix epoch.
    #[boyia_sync_builtin(method = "now")]
    fn now() -> usize {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as usize)
            .unwrap_or(0)
    }
}
