//! Plugin directory layout (host exe + env). Enable `host-exe-plugins` for `{exe}/plugins` default.

use std::path::PathBuf;

pub const ENV_ENABLE: &str = "BOYIA_ENABLE_DYLIB_BUILTINS";
pub const ENV_PLUGIN_DIRS: &str = "BOYIA_PLUGIN_DIRS";
pub const ENV_STRICT: &str = "BOYIA_PLUGIN_STRICT";

/// Subdirectory next to the host executable where dylib plugins are loaded from.
pub const HOST_PLUGINS_SUBDIR: &str = "plugins";

/// Default scan directories when `BOYIA_PLUGIN_DIRS` is unset.
pub fn default_plugin_dirs() -> Vec<PathBuf> {
    #[cfg(feature = "host-exe-plugins")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                return vec![dir.join(HOST_PLUGINS_SUBDIR)];
            }
        }
    }
    vec![PathBuf::from(HOST_PLUGINS_SUBDIR)]
}
