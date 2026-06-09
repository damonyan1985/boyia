//! File builtin: async IO on thread pool, callback on Boyia task thread.

#![allow(dead_code)]

use super::r#async::{
    attach_method, register_async_builtin_class, AsyncBuiltinResult, AsyncCtx, CallSite, ScriptCallback,
};
use crate::define_async_native;
use crate::some_or_end;
use boyia_vm::{LUintPtr, OpHandleResult, BoyiaVM};
use std::fs::{self, File};
use std::io::ErrorKind;

pub fn builtin_file_class<F>(vm: &mut BoyiaVM, gen_id: &mut F)
where
    F: FnMut(&str) -> LUintPtr,
{
    register_async_builtin_class(vm, gen_id, "File", |class_body, vm, gen_id| {
        attach_method(gen_id, "read", file_read_native, class_body, vm);
        attach_method(gen_id, "write", file_write_native, class_body, vm);
        attach_method(gen_id, "createDirs", file_create_dirs_native, class_body, vm);
        attach_method(gen_id, "create", file_create_native, class_body, vm);
        attach_method(gen_id, "delete", file_delete_native, class_body, vm);
        attach_method(gen_id, "exists", file_exists_native, class_body, vm);
    });
}

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

fn schedule_exists(ctx: &AsyncCtx, path: String, callback: ScriptCallback) -> bool {
    ctx.spawn(
        move || path_exists_result(&path),
        callback,
        |_| (),
    )
}

fn schedule_read(ctx: &AsyncCtx, path: String, callback: ScriptCallback) -> bool {
    ctx.spawn(
        move || match fs::read_to_string(&path) {
            Ok(text) => AsyncBuiltinResult::Ok {
                data: Some(text),
            },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.read error: {err}"),
            },
        },
        callback,
        |_| (),
    )
}

fn schedule_write(ctx: &AsyncCtx, path: String, content: String, callback: ScriptCallback) -> bool {
    ctx.spawn(
        move || match fs::write(&path, content.as_bytes()) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.write error: {err}"),
            },
        },
        callback,
        |_| (),
    )
}

fn schedule_create_dirs(ctx: &AsyncCtx, path: String, callback: ScriptCallback) -> bool {
    ctx.spawn(
        move || match fs::create_dir_all(&path) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.createDirs error: {err}"),
            },
        },
        callback,
        |_| (),
    )
}

fn schedule_create_file(ctx: &AsyncCtx, path: String, callback: ScriptCallback) -> bool {
    ctx.spawn(
        move || match File::create(&path) {
            Ok(_f) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.create error: {err}"),
            },
        },
        callback,
        |_| (),
    )
}

fn schedule_delete(ctx: &AsyncCtx, path: String, callback: ScriptCallback) -> bool {
    ctx.spawn(
        move || match fs::remove_file(&path) {
            Ok(()) => AsyncBuiltinResult::Ok { data: None },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.delete error: {err}"),
            },
        },
        callback,
        |_| (),
    )
}

fn file_read_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let path = some_or_end!(site.arg_string(1));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_read(site.ctx(), path, callback))
}

fn file_write_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let path = some_or_end!(site.arg_string(1));
    let content = some_or_end!(site.arg_string(2));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_write(site.ctx(), path, content, callback))
}

fn file_create_dirs_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let path = some_or_end!(site.arg_string(1));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_create_dirs(site.ctx(), path, callback))
}

fn file_create_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let path = some_or_end!(site.arg_string(1));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_create_file(site.ctx(), path, callback))
}

fn file_delete_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let path = some_or_end!(site.arg_string(1));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_delete(site.ctx(), path, callback))
}

fn file_exists_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let path = some_or_end!(site.arg_string(1));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_exists(site.ctx(), path, callback))
}

define_async_native!(file_read_native, 3, file_read_handler);
define_async_native!(file_write_native, 4, file_write_handler);
define_async_native!(file_create_dirs_native, 3, file_create_dirs_handler);
define_async_native!(file_create_native, 3, file_create_handler);
define_async_native!(file_delete_native, 3, file_delete_handler);
define_async_native!(file_exists_native, 3, file_exists_handler);
