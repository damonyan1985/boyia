//! File builtin: async IO on thread pool, callback on Boyia task thread.

#![allow(dead_code)]

use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_async_class;
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

#[boyia_async_class(name = "File", registrar = builtin_file_class)]
mod file_builtins {
    #[boyia_async_builtin(native = file_read_native, method = "read")]
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

    #[boyia_async_builtin(native = file_write_native, method = "write")]
    fn file_write(path: String, content: String) -> AsyncBuiltinResult {
        match fs::write(&path, content.as_bytes()) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.write error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(native = file_create_dirs_native, method = "createDirs")]
    fn file_create_dirs(path: String) -> AsyncBuiltinResult {
        match fs::create_dir_all(&path) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.createDirs error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(native = file_create_native, method = "create")]
    fn file_create(path: String) -> AsyncBuiltinResult {
        match File::create(&path) {
            Ok(_f) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.create error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(native = file_delete_native, method = "delete")]
    fn file_delete(path: String) -> AsyncBuiltinResult {
        match fs::remove_file(&path) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.delete error: {err}"),
            },
        }
    }

    #[boyia_async_builtin(native = file_exists_native, method = "exists")]
    fn file_exists(path: String) -> AsyncBuiltinResult {
        path_exists_result(&path)
    }

    #[boyia_sync_builtin(native = file_is_absolute_native, method = "isAbsolute")]
    fn file_is_absolute(path: String) -> bool {
        std::path::Path::new(&path).is_absolute()
    }
}
