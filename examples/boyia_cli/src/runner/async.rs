//! Async builtin infrastructure: VM `unsafe` is confined here.
//! Business modules (`file` / `https` / `zip`) use [CallSite], [AsyncCtx], [ScriptCallback] only.

use boyia_builtins::gen_builtin_class_function;
use crate::runner::builtin_json::json_to_boyia_value;
use boyia_runtime::{boyia_runtime_from_vm, BoyiaRuntime};
use boyia_vm::{
    copy_object, create_global_class, create_native_string, create_string_object, gen_identifier_from_str,
    get_function_count, get_local_size, get_local_value, native_call_impl, set_int_result, value_copy,
    vector_params_grow_if_full, set_native_result, vm_from_void, BoyiaClass, BoyiaFunction, BoyiaStr,
    BoyiaValue, BuiltinId, Global, K_BOYIA_NULL, NativePtr, OpHandleResult, RealValue, Runtime, ValueType,
    LInt, LInt8, LIntPtr, LUintPtr, BoyiaVM,
};
use super::run_loop::RunLoopHandle;
use super::thread_pool::ThreadPool;
use std::str;
use std::sync::Weak;

/// Stored on [BoyiaRuntime] via embedder during CLI init.
#[derive(Clone)]
pub struct CliEmbedder {
    pub async_ctx: AsyncCtx,
}

/// Safe handle for scheduling thread-pool work and posting callbacks to the Boyia task thread.
#[derive(Clone)]
pub struct AsyncCtx {
    runtime_handle: RunLoopHandle<Box<BoyiaRuntime>>,
    thread_pool: Weak<ThreadPool>,
}

impl AsyncCtx {
    pub fn new(runtime_handle: RunLoopHandle<Box<BoyiaRuntime>>, thread_pool: Weak<ThreadPool>) -> Self {
        Self {
            runtime_handle,
            thread_pool,
        }
    }

    pub fn spawn<W, H>(&self, work: W, callback: ScriptCallback, before_callback: H) -> bool
    where
        W: FnOnce() -> AsyncBuiltinResult + Send + 'static,
        H: FnOnce(&AsyncBuiltinResult) + Send + 'static,
    {
        let Some(thread_pool) = self.thread_pool.upgrade() else {
            return false;
        };
        let runtime_handle = self.runtime_handle.clone();
        let cb = callback.0;
        thread_pool
            .post_task(move || {
                let body = work();
                let _ = runtime_handle.post_task(move |runtime| {
                    before_callback(&body);
                    unsafe {
                        callback_async_result(body, cb, runtime.as_mut());
                    }
                    runtime.consume_micro_task();
                });
            })
            .is_ok()
    }
}

/// Opaque script callback token (VM details live inside [CallbackInfo]).
#[derive(Clone)]
pub struct ScriptCallback(CallbackInfo);

/// Result posted to script callbacks: a Map with `status`, optional `data` / `message`.
#[derive(Debug)]
pub enum AsyncBuiltinResult {
    Ok { data: Option<String> },
    /// Parsed JSON converted to a Boyia Map/Array/String/number on the VM thread.
    OkJson(serde_json::Value),
    Fail { message: String },
}

impl AsyncBuiltinResult {
    pub fn log_preview(&self) -> &str {
        match self {
            AsyncBuiltinResult::Ok { data: Some(d) } => d.as_str(),
            AsyncBuiltinResult::Ok { data: None } => "",
            AsyncBuiltinResult::OkJson(_) => "<json>",
            AsyncBuiltinResult::Fail { message } => message.as_str(),
        }
    }
}

/// Safe view of VM locals for one native call.
pub struct CallSite<'a> {
    vm: &'a mut BoyiaVM,
    size: LInt,
    ctx: AsyncCtx,
}

impl<'a> CallSite<'a> {
    pub fn open(vm: &'a mut BoyiaVM, min_locals: LInt) -> Option<Self> {
        let size = unsafe { get_local_size(vm) };
        if size < min_locals {
            return None;
        }
        let ctx = async_ctx_from_vm(vm)?;
        Some(Self { vm, size, ctx })
    }

    pub fn ctx(&self) -> &AsyncCtx {
        &self.ctx
    }

    pub fn vm(&mut self) -> &mut BoyiaVM {
        self.vm
    }

    pub fn arg_boyia_value(&mut self, index: LInt) -> Option<*const BoyiaValue> {
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        if val.is_null() {
            None
        } else {
            Some(val)
        }
    }

    pub fn arg_string(&mut self, index: LInt) -> Option<String> {
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        value_to_string(val)
    }

    /// Optional middle arg: use `default` when `index` is absent (e.g. Zip password with 5 locals).
    pub fn arg_string_or(&mut self, index: LInt, default: &str) -> String {
        if self.size > index {
            self.arg_string(index).unwrap_or_else(|| default.to_string())
        } else {
            default.to_string()
        }
    }

    /// Callback is always the local immediately before `this` (`size - 2`).
    pub fn callback(&mut self) -> Option<ScriptCallback> {
        let index = self.size - 2;
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        unsafe { make_callback_info(self.vm, val) }.map(ScriptCallback)
    }

    pub fn finish(&mut self, scheduled: bool) -> OpHandleResult {
        unsafe {
            set_int_result(if scheduled { 1 } else { 0 }, self.vm);
        }
        OpHandleResult::kOpResultSuccess
    }
}

fn async_ctx_from_vm(vm: &mut BoyiaVM) -> Option<AsyncCtx> {
    unsafe {
        boyia_runtime_from_vm(vm)?
            .embedder::<CliEmbedder>()
            .map(|e| e.async_ctx.clone())
    }
}

pub fn async_dispatch(
    vm: &mut BoyiaVM,
    min_locals: LInt,
    handler: fn(&mut CallSite<'_>) -> OpHandleResult,
) -> OpHandleResult {
    let Some(mut site) = CallSite::open(vm, min_locals) else {
        return OpHandleResult::kOpResultEnd;
    };
    handler(&mut site)
}

pub fn attach_method<F>(
    gen_id: &mut F,
    name: &str,
    native: NativePtr,
    class_body: *mut BoyiaFunction,
    vm: &mut BoyiaVM,
) where
    F: FnMut(&str) -> LUintPtr + ?Sized,
{
    unsafe {
        gen_builtin_class_function(gen_id(name), native, class_body, vm);
    }
}

/// Create global class and register native methods (no runner field on the class).
pub fn register_async_builtin_class<F, R>(
    vm: &mut BoyiaVM,
    gen_id: &mut F,
    class_name: &str,
    mut register: R,
) where
    F: FnMut(&str) -> LUintPtr + ?Sized,
    R: FnMut(*mut BoyiaFunction, &mut BoyiaVM, &mut F),
{
    let class_key = gen_id(class_name);
    let class_ref = unsafe { create_global_class(class_key, vm) } as *mut BoyiaValue;
    if class_ref.is_null() {
        return;
    }
    unsafe {
        (*class_ref).mValue.mObj.mSuper = K_BOYIA_NULL;
        let class_body = (*class_ref).mValue.mObj.mPtr as *mut BoyiaFunction;
        register(class_body, vm, gen_id);
    }
}

#[macro_export]
macro_rules! define_async_native {
    ($native:ident, $min:expr, $handler:ident) => {
        unsafe fn $native(vm: &mut boyia_vm::BoyiaVM) -> boyia_vm::OpHandleResult {
            $crate::runner::r#async::async_dispatch(vm, $min, $handler)
        }
    };
}

#[macro_export]
macro_rules! some_or_end {
    ($e:expr) => {
        match $e {
            Some(value) => value,
            None => return boyia_vm::OpHandleResult::kOpResultEnd,
        }
    };
}

#[derive(Clone, Copy)]
struct CallbackInfo {
    name_key: LUintPtr,
    value_type: ValueType,
    func_ptr: LIntPtr,
    object_global: *mut Global,
}

unsafe impl Send for CallbackInfo {}

fn value_to_string(value: *const BoyiaValue) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let str_ref = unsafe { boyia_vm::get_string_buffer(value) };
    if str_ref.is_null() {
        return None;
    }
    let len = unsafe { (*str_ref).mLen.max(0) as usize };
    let ptr = unsafe { (*str_ref).mPtr as *const u8 };
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    str::from_utf8(slice).ok().map(ToOwned::to_owned)
}

unsafe fn make_callback_info(vm: &mut BoyiaVM, callback_val: *const BoyiaValue) -> Option<CallbackInfo> {
    if callback_val.is_null() {
        return None;
    }
    let object_addr = (*callback_val).mValue.mObj.mSuper;
    let object_value = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_CLASS,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: object_addr,
                mSuper: K_BOYIA_NULL,
            },
        },
    };
    let object_global = {
        let rt = boyia_vm::get_runtime_from_vm(vm);
        if rt.is_null() {
            std::ptr::null_mut()
        } else {
            (*rt).persistent_object(&object_value as *const BoyiaValue)
        }
    };
    Some(CallbackInfo {
        name_key: (*callback_val).mNameKey,
        value_type: (*callback_val).mValueType,
        func_ptr: (*callback_val).mValue.mObj.mPtr,
        object_global,
    })
}

unsafe fn native_string_value(vm: &mut BoyiaVM, s: &str) -> Option<BoyiaValue> {
    if s.is_empty() {
        let body = create_string_object(std::ptr::null_mut(), 0, vm);
        if body.is_null() {
            return None;
        }
        return Some(BoyiaValue {
            mNameKey: BuiltinId::kBoyiaString.as_key(),
            mValueType: ValueType::BY_CLASS,
            mValue: RealValue {
                mObj: BoyiaClass {
                    mPtr: body as LIntPtr,
                    mSuper: K_BOYIA_NULL,
                },
            },
        });
    }
    let boxed = s.as_bytes().to_vec().into_boxed_slice();
    let len = boxed.len() as LInt;
    let ptr = Box::into_raw(boxed) as *mut u8 as *mut LInt8;
    let mut value = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_INT,
        mValue: RealValue { mIntVal: 0 },
    };
    create_native_string(&mut value, ptr, len, vm);
    if value.mValue.mObj.mPtr == K_BOYIA_NULL {
        return None;
    }
    Some(value)
}

unsafe fn map_put_boyia_key(vm: &mut BoyiaVM, map_obj: *mut BoyiaValue, key: &str, val: &BoyiaValue) -> bool {
    let fun = (*map_obj).mValue.mObj.mPtr as *mut BoyiaFunction;
    if fun.is_null() || (*fun).mParams.is_null() {
        return false;
    }
    let kb = key.as_bytes();
    let bstr = BoyiaStr {
        mPtr: kb.as_ptr() as *mut LInt8,
        mLen: kb.len() as LInt,
    };
    let key_id = gen_identifier_from_str(vm, &bstr);
    let cap = get_function_count(fun);
    if (*fun).mParamSize >= cap && !vector_params_grow_if_full(fun, vm) {
        return false;
    }
    let slot = (*fun).mParams.add((*fun).mParamSize as usize);
    value_copy(slot, val);
    (*slot).mNameKey = key_id;
    (*fun).mParamSize += 1;
    true
}

unsafe fn map_put_str_key(vm: &mut BoyiaVM, map_obj: *mut BoyiaValue, key: &str, val: &BoyiaValue) -> bool {
    let fun = (*map_obj).mValue.mObj.mPtr as *mut BoyiaFunction;
    if fun.is_null() || (*fun).mParams.is_null() {
        return false;
    }
    let kb = key.as_bytes();
    let bstr = BoyiaStr {
        mPtr: kb.as_ptr() as *mut LInt8,
        mLen: kb.len() as LInt,
    };
    let key_id = gen_identifier_from_str(vm, &bstr);
    let cap = get_function_count(fun);
    if (*fun).mParamSize >= cap && !vector_params_grow_if_full(fun, vm) {
        return false;
    }
    let slot = (*fun).mParams.add((*fun).mParamSize as usize);
    value_copy(slot, val);
    (*slot).mNameKey = key_id;
    (*fun).mParamSize += 1;
    true
}

unsafe fn build_async_result_map(vm: &mut BoyiaVM, r: &AsyncBuiltinResult) -> Option<BoyiaValue> {
    let raw = copy_object(BuiltinId::kBoyiaMap.as_key(), 32, vm);
    if raw.is_null() {
        return None;
    }
    let mut map_val = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_CLASS,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: raw as LIntPtr,
                mSuper: K_BOYIA_NULL,
            },
        },
    };
    set_native_result(&mut map_val, vm);

    let status_s = match r {
        AsyncBuiltinResult::Ok { .. } | AsyncBuiltinResult::OkJson(_) => "ok",
        AsyncBuiltinResult::Fail { .. } => "fail",
    };
    let status_val = native_string_value(vm, status_s)?;
    if !map_put_str_key(vm, &mut map_val, "status", &status_val) {
        return None;
    }

    match r {
        AsyncBuiltinResult::Ok { data: Some(d) } => {
            let dv = native_string_value(vm, d)?;
            if !map_put_str_key(vm, &mut map_val, "data", &dv) {
                return None;
            }
        }
        AsyncBuiltinResult::Ok { data: None } => {}
        AsyncBuiltinResult::OkJson(j) => {
            let mut bv = match json_to_boyia_value(vm, j) {
                Ok(v) => v,
                Err(_) => return None,
            };
            if !map_put_boyia_key(vm, &mut map_val, "data", &mut bv) {
                return None;
            }
        }
        AsyncBuiltinResult::Fail { message } => {
            let mv = native_string_value(vm, message)?;
            if !map_put_str_key(vm, &mut map_val, "message", &mv) {
                return None;
            }
        }
    }
    Some(map_val)
}

unsafe fn callback_async_result(
    result: AsyncBuiltinResult,
    callback: CallbackInfo,
    runtime: &mut BoyiaRuntime,
) {
    let vm_ptr = runtime.vm();
    if vm_ptr.is_null() {
        if !callback.object_global.is_null() {
            runtime.remove_persistent(callback.object_global);
        }
        return;
    }
    let vm = vm_from_void(vm_ptr).expect("runtime VM");
    let Some(map_val) = build_async_result_map(vm, &result) else {
        if !callback.object_global.is_null() {
            runtime.remove_persistent(callback.object_global);
        }
        return;
    };

    let cb_fun = callback.func_ptr as *mut BoyiaFunction;
    if cb_fun.is_null() {
        if !callback.object_global.is_null() {
            runtime.remove_persistent(callback.object_global);
        }
        return;
    }

    let obj_super = if callback.object_global.is_null() {
        K_BOYIA_NULL
    } else {
        (*callback.object_global).value().mValue.mObj.mPtr
    };

    let callback_value = BoyiaValue {
        mNameKey: callback.name_key,
        mValueType: callback.value_type,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: callback.func_ptr,
                mSuper: obj_super,
            },
        },
    };

    let mut args = [callback_value, map_val];
    if !(*cb_fun).mParams.is_null() {
        args[1].mNameKey = (*(*cb_fun).mParams).mNameKey;
    }

    let mut obj = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_CLASS,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: obj_super,
                mSuper: K_BOYIA_NULL,
            },
        },
    };
    native_call_impl(args.as_mut_ptr(), 2, &mut obj, vm);

    if !callback.object_global.is_null() {
        runtime.remove_persistent(callback.object_global);
    }
}
