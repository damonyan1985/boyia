//! File builtin: async IO on thread pool, callback on Boyia task thread.

#![allow(dead_code)]

use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use std::fs::{self, File};
use std::io::ErrorKind;

fn path_exists_result(path: &str) -> AsyncBuiltinResult {
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
        match fs::read_to_string(&path) {
            Ok(text) => AsyncBuiltinResult::Ok {
                data: Some(text),
            },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.read error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(method = "write")]
    fn file_write(path: String, content: String) -> AsyncBuiltinResult {
        match fs::write(&path, content.as_bytes()) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.write error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(method = "createDirs")]
    fn file_create_dirs(path: String) -> AsyncBuiltinResult {
        match fs::create_dir_all(&path) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.createDirs error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(method = "create")]
    fn file_create(path: String) -> AsyncBuiltinResult {
        match File::create(&path) {
            Ok(_f) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.create error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(method = "delete")]
    fn file_delete(path: String) -> AsyncBuiltinResult {
        match fs::remove_file(&path) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.delete error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(method = "exists")]
    fn file_exists(path: String) -> AsyncBuiltinResult {
        path_exists_result(&path)
    }

    #[boyia_sync_builtin(method = "isAbsolute")]
    fn file_is_absolute(path: String) -> bool {
        std::path::Path::new(&path).is_absolute()
    }
}
