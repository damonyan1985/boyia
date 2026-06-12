//! Sync builtin infrastructure: safe arg extraction and Rust return → BoyiaValue.

use boyia_vm::{
    create_native_string, create_string_object, get_local_size, get_local_value, set_int_result,
    set_native_result, BoyiaClass, BoyiaFunction, BoyiaValue, BuiltinId, K_BOYIA_NULL,
    OpHandleResult, RealValue, ValueType, LInt, LInt8, LIntPtr, BoyiaVM,
};
use std::str;

/// Safe view of VM locals for one synchronous native call (no callback local).
pub struct SyncCallSite<'a> {
    vm: &'a mut BoyiaVM,
    size: LInt,
}

impl<'a> SyncCallSite<'a> {
    pub fn open(vm: &'a mut BoyiaVM, min_locals: LInt) -> Option<Self> {
        let size = unsafe { get_local_size(vm) };
        if size < min_locals {
            return None;
        }
        Some(Self { vm, size })
    }

    pub fn vm(&mut self) -> &mut BoyiaVM {
        self.vm
    }

    /// `this` for `BY_NAV_FUNC` builtins: last local pushed before the native runs.
    pub fn this_function(&mut self) -> Option<*mut BoyiaFunction> {
        let val = unsafe { get_local_value(self.size - 1, self.vm) as *const BoyiaValue };
        if val.is_null() {
            return None;
        }
        unsafe {
            let ptr = (*val).mValue.mObj.mPtr;
            if ptr == K_BOYIA_NULL {
                return None;
            }
            Some(ptr as *mut BoyiaFunction)
        }
    }

    pub fn arg_string(&mut self, index: LInt) -> Option<String> {
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        value_to_string(val)
    }

    pub fn arg_string_or(&mut self, index: LInt, default: &str) -> String {
        if self.size > index {
            self.arg_string(index).unwrap_or_else(|| default.to_string())
        } else {
            default.to_string()
        }
    }

    pub fn arg_bool(&mut self, index: LInt) -> Option<bool> {
        self.arg_int(index).map(|n| n != 0)
    }

    pub fn arg_i32(&mut self, index: LInt) -> Option<i32> {
        self.arg_int(index).map(|n| n as i32)
    }

    pub fn arg_i64(&mut self, index: LInt) -> Option<i64> {
        self.arg_int(index)
    }

    pub fn arg_f64(&mut self, index: LInt) -> Option<f64> {
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        if val.is_null() {
            return None;
        }
        let v = unsafe { &*val };
        match v.mValueType {
            ValueType::BY_REAL => Some(unsafe { v.mValue.mRealVal }),
            ValueType::BY_INT | ValueType::BY_CHAR => Some(unsafe { v.mValue.mIntVal as f64 }),
            _ => None,
        }
    }

    fn arg_int(&mut self, index: LInt) -> Option<i64> {
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        if val.is_null() {
            return None;
        }
        let v = unsafe { &*val };
        match v.mValueType {
            ValueType::BY_INT | ValueType::BY_CHAR => Some(unsafe { v.mValue.mIntVal as i64 }),
            ValueType::BY_REAL => Some(unsafe { v.mValue.mRealVal as i64 }),
            _ => None,
        }
    }
}

pub fn sync_dispatch(
    vm: &mut BoyiaVM,
    min_locals: LInt,
    handler: fn(&mut SyncCallSite<'_>) -> OpHandleResult,
) -> OpHandleResult {
    let Some(mut site) = SyncCallSite::open(vm, min_locals) else {
        return OpHandleResult::kOpResultEnd;
    };
    handler(&mut site)
}

/// Convert a sync builtin Rust return value into VM reg0.
pub trait SyncReturn {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult;
}

pub fn set_sync_return<R: SyncReturn>(result: R, vm: &mut BoyiaVM) -> OpHandleResult {
    result.set_result(vm)
}

impl SyncReturn for () {
    fn set_result(self, _vm: &mut BoyiaVM) -> OpHandleResult {
        OpHandleResult::kOpResultSuccess
    }
}

impl SyncReturn for bool {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
        unsafe {
            set_int_result(if self { 1 } else { 0 }, vm);
        }
        OpHandleResult::kOpResultSuccess
    }
}

macro_rules! impl_sync_int_return {
    ($($ty:ty),+) => {
        $(
            impl SyncReturn for $ty {
                fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
                    unsafe {
                        set_int_result(self as LInt, vm);
                    }
                    OpHandleResult::kOpResultSuccess
                }
            }
        )+
    };
}

impl_sync_int_return!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl SyncReturn for f32 {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
        (self as f64).set_result(vm)
    }
}

impl SyncReturn for f64 {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
        let mut value = BoyiaValue {
            mNameKey: 0,
            mValueType: ValueType::BY_REAL,
            mValue: RealValue { mRealVal: self },
        };
        unsafe {
            set_native_result(&mut value, vm);
        }
        OpHandleResult::kOpResultSuccess
    }
}

impl SyncReturn for String {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
        match unsafe { string_to_boyia_value(vm, &self) } {
            Some(mut value) => {
                unsafe {
                    set_native_result(&mut value, vm);
                }
                OpHandleResult::kOpResultSuccess
            }
            None => OpHandleResult::kOpResultEnd,
        }
    }
}

impl SyncReturn for Option<String> {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
        match self {
            Some(s) => s.set_result(vm),
            None => {
                unsafe {
                    set_int_result(0, vm);
                }
                OpHandleResult::kOpResultSuccess
            }
        }
    }
}

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

unsafe fn string_to_boyia_value(vm: &mut BoyiaVM, s: &str) -> Option<BoyiaValue> {
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

#[macro_export]
macro_rules! define_sync_native {
    ($native:ident, $min:expr, $handler:ident) => {
        unsafe fn $native(vm: &mut boyia_vm::BoyiaVM) -> boyia_vm::OpHandleResult {
            $crate::runner::sync::sync_dispatch(vm, $min, $handler)
        }
    };
}
