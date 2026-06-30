//! OS builtin: process working directory and platform introspection.

#![allow(dead_code)]

use builtin_macro::boyia_class;
use std::env;
use std::time::Duration;

struct OsBuiltins;

#[boyia_class(name = "OS", registrar = builtin_os_class)]
impl OsBuiltins {
    #[boyia_sync_builtin(method = "cwd")]
    fn os_cwd() -> String {
        env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// User home directory (`$HOME` on Unix, `%USERPROFILE%` on Windows).
    #[boyia_sync_builtin(method = "home")]
    fn os_home() -> String {
        env::var_os("HOME")
            .or_else(|| env::var_os("USERPROFILE"))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    #[boyia_sync_builtin(method = "chdir")]
    fn os_chdir(path: String) -> bool {
        env::set_current_dir(path).is_ok()
    }

    #[boyia_sync_builtin(method = "name")]
    fn os_name() -> String {
        env::consts::OS.to_string()
    }

    #[boyia_sync_builtin(method = "cpuCount")]
    fn os_cpu_count() -> usize {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    }

    /// Sleep current thread for `ms` milliseconds.
    #[boyia_sync_builtin(method = "sleep")]
    fn os_sleep(ms: u64) -> bool {
        std::thread::sleep(Duration::from_millis(ms));
        true
    }
}
