//! Path builtin: lexical path joining, no filesystem access.

#![allow(dead_code)]

use boyia_plugin_sdk::boyia_class;
use std::path::{Component, Path, PathBuf};

fn path_normalize(path: &str) -> String {
    let mut out = PathBuf::new();
    for component in Path::new(path).components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out.to_string_lossy().into_owned()
}

fn path_join(base: &str, rel: &str) -> String {
    if rel.is_empty() {
        return base.to_string();
    }
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        return path_normalize(rel);
    }
    let joined = Path::new(base).join(rel_path);
    path_normalize(&joined.to_string_lossy())
}

struct PathPluginBuiltins;

#[boyia_class(name = "PathPlugin", registrar = builtin_path_plugin_class)]
impl PathPluginBuiltins {
    /// Join `rel` onto `base` and normalize `.` / `..`; absolute `rel` replaces `base`.
    #[boyia_sync_builtin(method = "join")]
    fn path_join_builtin(base: String, rel: String) -> String {
        path_join(&base, &rel)
    }
}
