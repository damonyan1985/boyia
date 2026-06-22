//! File builtin: async IO on thread pool, callback on Boyia task thread.
//! Sync variants (`*Sync`) run on the VM thread with plain return values.

#![allow(dead_code)]

use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

fn normalize_path_key(path: &str) -> String {
    let p = Path::new(path);
    fs::canonicalize(p)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string())
}

fn path_base_dir(base: &str) -> PathBuf {
    if base.is_empty() {
        return std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    }
    let b = Path::new(base);
    if b.is_dir() {
        b.to_path_buf()
    } else {
        b.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn path_join(base: &str, rel: &str) -> String {
    if rel.is_empty() {
        return base.to_string();
    }
    let rel_p = Path::new(rel);
    if rel_p.is_absolute() {
        return rel_p.to_string_lossy().into_owned();
    }
    let joined = path_base_dir(base).join(rel_p);
    path_normalize(&joined.to_string_lossy())
}

fn path_resolve(base: &str, rel: &str) -> String {
    let rel_p = Path::new(rel);
    if rel_p.is_absolute() {
        return normalize_path_key(rel);
    }
    let joined = path_base_dir(base).join(rel_p);
    normalize_path_key(&joined.to_string_lossy())
}

fn path_dirname(path: &str) -> String {
    let p = Path::new(path);
    p.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_else(|| ".".to_string())
}

fn path_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

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
    if out.as_os_str().is_empty() {
        ".".to_string()
    } else {
        out.to_string_lossy().into_owned()
    }
}

fn file_read_sync(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn file_write_sync(path: &str, content: &str) -> bool {
    fs::write(path, content.as_bytes()).is_ok()
}

fn file_create_dirs_sync(path: &str) -> bool {
    fs::create_dir_all(path).is_ok()
}

fn file_create_sync(path: &str) -> bool {
    File::create(path).is_ok()
}

fn file_delete_sync(path: &str) -> bool {
    fs::remove_file(path).is_ok()
}

fn file_exists_sync(path: &str) -> Option<String> {
    match fs::metadata(path) {
        Ok(meta) => {
            let tag = if meta.is_dir() {
                "dir"
            } else if meta.is_file() {
                "file"
            } else {
                "other"
            };
            Some(tag.to_string())
        }
        Err(_) => None,
    }
}

fn file_read_async(path: &str) -> AsyncBuiltinResult {
    match fs::read_to_string(path) {
        Ok(text) => AsyncBuiltinResult::Ok {
            data: Some(text),
        },
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("File.read error: {err}"),
        },
    }
}

fn file_write_async(path: &str, content: &str) -> AsyncBuiltinResult {
    match fs::write(path, content.as_bytes()) {
        Ok(()) => AsyncBuiltinResult::Ok { data: None },
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("File.write error: {err}"),
        },
    }
}

fn file_create_dirs_async(path: &str) -> AsyncBuiltinResult {
    match fs::create_dir_all(path) {
        Ok(()) => AsyncBuiltinResult::Ok { data: None },
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("File.createDirs error: {err}"),
        },
    }
}

fn file_create_async(path: &str) -> AsyncBuiltinResult {
    match File::create(path) {
        Ok(_f) => AsyncBuiltinResult::Ok { data: None },
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("File.create error: {err}"),
        },
    }
}

fn file_delete_async(path: &str) -> AsyncBuiltinResult {
    match fs::remove_file(path) {
        Ok(()) => AsyncBuiltinResult::Ok { data: None },
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("File.delete error: {err}"),
        },
    }
}

fn file_exists_async(path: &str) -> AsyncBuiltinResult {
    match fs::metadata(path) {
        Ok(meta) => {
            let tag = if meta.is_dir() {
                "dir"
            } else if meta.is_file() {
                "file"
            } else {
                "other"
            };
            AsyncBuiltinResult::Ok {
                data: Some(tag.to_string()),
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => AsyncBuiltinResult::Fail {
            message: format!("File.exists: not found ({e})"),
        },
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("File.exists error: {err}"),
        },
    }
}

struct FileBuiltins;

#[boyia_class(name = "File", registrar = builtin_file_class)]
impl FileBuiltins {
    #[boyia_async_builtin(method = "read")]
    fn file_read(path: String) -> AsyncBuiltinResult {
        file_read_async(&path)
    }

    #[boyia_sync_builtin(method = "readSync")]
    fn file_read_sync_builtin(path: String) -> Option<String> {
        file_read_sync(&path)
    }

    #[boyia_async_builtin(method = "write")]
    fn file_write(path: String, content: String) -> AsyncBuiltinResult {
        file_write_async(&path, &content)
    }

    #[boyia_sync_builtin(method = "writeSync")]
    fn file_write_sync_builtin(path: String, content: String) -> bool {
        file_write_sync(&path, &content)
    }

    #[boyia_async_builtin(method = "createDirs")]
    fn file_create_dirs(path: String) -> AsyncBuiltinResult {
        file_create_dirs_async(&path)
    }

    #[boyia_sync_builtin(method = "createDirsSync")]
    fn file_create_dirs_sync_builtin(path: String) -> bool {
        file_create_dirs_sync(&path)
    }

    #[boyia_async_builtin(method = "create")]
    fn file_create(path: String) -> AsyncBuiltinResult {
        file_create_async(&path)
    }

    #[boyia_sync_builtin(method = "createSync")]
    fn file_create_sync_builtin(path: String) -> bool {
        file_create_sync(&path)
    }

    #[boyia_async_builtin(method = "delete")]
    fn file_delete(path: String) -> AsyncBuiltinResult {
        file_delete_async(&path)
    }

    #[boyia_sync_builtin(method = "deleteSync")]
    fn file_delete_sync_builtin(path: String) -> bool {
        file_delete_sync(&path)
    }

    #[boyia_async_builtin(method = "exists")]
    fn file_exists(path: String) -> AsyncBuiltinResult {
        file_exists_async(&path)
    }

    #[boyia_sync_builtin(method = "existsSync")]
    fn file_exists_sync_builtin(path: String) -> Option<String> {
        file_exists_sync(&path)
    }

    #[boyia_sync_builtin(method = "isAbsolute")]
    fn file_is_absolute(path: String) -> bool {
        Path::new(&path).is_absolute()
    }

    #[boyia_sync_builtin(method = "join")]
    fn file_join(base: String, rel: String) -> String {
        path_join(&base, &rel)
    }

    #[boyia_sync_builtin(method = "resolve")]
    fn file_resolve(base: String, rel: String) -> String {
        path_resolve(&base, &rel)
    }

    #[boyia_sync_builtin(method = "dirname")]
    fn file_dirname(path: String) -> String {
        path_dirname(&path)
    }

    #[boyia_sync_builtin(method = "basename")]
    fn file_basename(path: String) -> String {
        path_basename(&path)
    }

    #[boyia_sync_builtin(method = "normalize")]
    fn file_normalize(path: String) -> String {
        path_normalize(&path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_relative_to_file_base() {
        let got = path_join("/proj/script/main.boyia", "./util/util.boyia");
        assert!(got.ends_with("script/util/util.boyia"));
    }

    #[test]
    fn resolve_uses_parent_of_file_base() {
        let got = path_resolve("/proj/script/main.boyia", "./ai.boyia");
        assert!(got.contains("script"));
        assert!(got.contains("ai.boyia"));
    }

    #[test]
    fn dirname_and_basename() {
        assert_eq!(path_dirname("/a/b/c.boyia"), "/a/b");
        assert_eq!(path_basename("/a/b/c.boyia"), "c.boyia");
    }

    #[test]
    fn read_sync_roundtrip() {
        let dir = std::env::temp_dir().join(format!("boyia_file_sync_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        assert!(file_write_sync(path.to_str().unwrap(), "hello"));
        assert_eq!(
            file_read_sync(path.to_str().unwrap()).as_deref(),
            Some("hello")
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn exists_sync_reports_kind() {
        let dir = std::env::temp_dir();
        assert_eq!(file_exists_sync(dir.to_str().unwrap()).as_deref(), Some("dir"));
        assert!(file_exists_sync("/no/such/boyia/path").is_none());
    }
}
