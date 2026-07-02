//! Sync builtin infrastructure: safe arg extraction and Rust return → BoyiaValue.

use boyia_vm::{
    copy_object, create_native_string, create_string_object, get_boyia_class_id, get_function_count,
    get_local_size, get_local_value, set_int_result, set_native_result, value_copy,
    vector_params_grow_if_full, BoyiaClass, BoyiaFunction, BoyiaValue, BuiltinId, K_BOYIA_NULL,
    OpHandleResult, RealValue, ValueType, LInt, LInt8, LIntPtr, BoyiaVM,
};
use std::hash::{Hash, Hasher};
use std::str;

/// Script value: integer, string, or array of integers/strings (used by HashMap and similar builtins).
#[derive(Clone, Debug)]
pub enum BoyiaScalar {
    Int(i64),
    Str(String),
    Arr(Vec<BoyiaScalar>),
}

impl PartialEq for BoyiaScalar {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Arr(a), Self::Arr(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for BoyiaScalar {}

impl Hash for BoyiaScalar {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Int(v) => {
                0u8.hash(state);
                v.hash(state);
            }
            Self::Str(v) => {
                1u8.hash(state);
                v.hash(state);
            }
            Self::Arr(v) => {
                2u8.hash(state);
                v.hash(state);
            }
        }
    }
}

impl BoyiaScalar {
    pub fn missing_default_for_key(key: &Self) -> Self {
        match key {
            Self::Int(_) => Self::Int(0),
            Self::Str(_) => Self::Str(String::new()),
            Self::Arr(_) => Self::Arr(Vec::new()),
        }
    }
}

pub fn set_sync_vec_scalar_return(items: Vec<BoyiaScalar>, vm: &mut BoyiaVM) -> OpHandleResult {
    unsafe {
        match scalars_to_boyia_array(vm, items) {
            Ok(mut out) => {
                set_native_result(&mut out, vm);
                OpHandleResult::kOpResultSuccess
            }
            Err(_) => OpHandleResult::kOpResultEnd,
        }
    }
}

unsafe fn scalars_to_boyia_array(vm: &mut BoyiaVM, items: Vec<BoyiaScalar>) -> Result<BoyiaValue, ()> {
    let raw = copy_object(BuiltinId::kBoyiaArray.as_key(), 32, vm);
    if raw.is_null() {
        return Err(());
    }
    let mut out = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_CLASS,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: raw as LIntPtr,
                mSuper: K_BOYIA_NULL,
            },
        },
    };
    for item in items {
        let elem = scalar_to_boyia_value(vm, item)?;
        array_add(vm, &mut out, &elem)?;
    }
    Ok(out)
}

unsafe fn array_add(vm: &mut BoyiaVM, arr_obj: *mut BoyiaValue, val: &BoyiaValue) -> Result<(), ()> {
    let fun = (*arr_obj).mValue.mObj.mPtr as *mut BoyiaFunction;
    if fun.is_null() {
        return Err(());
    }
    let cap = get_function_count(fun);
    if (*fun).mParamSize >= cap && !vector_params_grow_if_full(fun, vm) {
        return Err(());
    }
    let dst = (*fun).mParams.add((*fun).mParamSize as usize);
    value_copy(dst, val);
    (*dst).mNameKey = 0;
    (*fun).mParamSize += 1;
    Ok(())
}

unsafe fn scalar_to_boyia_value(vm: &mut BoyiaVM, scalar: BoyiaScalar) -> Result<BoyiaValue, ()> {
    match scalar {
        BoyiaScalar::Int(n) => Ok(BoyiaValue {
            mNameKey: 0,
            mValueType: ValueType::BY_INT,
            mValue: RealValue {
                mIntVal: n as LIntPtr,
            },
        }),
        BoyiaScalar::Str(s) => string_to_boyia_value(vm, &s).ok_or(()),
        BoyiaScalar::Arr(items) => scalars_to_boyia_array(vm, items),
    }
}

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

    /// Callback local for tuple-return sync builtins: `size - 2` (before `this`).
    pub fn capture_callback(&mut self) -> Option<crate::runner::builtin_async::ScriptCallback> {
        crate::runner::builtin_async::capture_script_callback(self.vm, self.size - 2)
    }

    pub fn arg_scalar(&mut self, index: LInt) -> Option<BoyiaScalar> {
        let val = unsafe { get_local_value(index, self.vm) as *const BoyiaValue };
        if val.is_null() {
            return None;
        }
        if let Some(items) = unsafe { value_to_scalar_array(val) } {
            return Some(BoyiaScalar::Arr(items));
        }
        if let Some(s) = value_to_string(val) {
            return Some(BoyiaScalar::Str(s));
        }
        self.arg_int(index).map(BoyiaScalar::Int)
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

impl SyncReturn for BoyiaScalar {
    fn set_result(self, vm: &mut BoyiaVM) -> OpHandleResult {
        match self {
            BoyiaScalar::Int(n) => n.set_result(vm),
            BoyiaScalar::Str(s) => s.set_result(vm),
            BoyiaScalar::Arr(items) => set_sync_vec_scalar_return(items, vm),
        }
    }
}

unsafe fn value_to_scalar_array(value: *const BoyiaValue) -> Option<Vec<BoyiaScalar>> {
    if value.is_null() || (*value).mValueType != ValueType::BY_CLASS {
        return None;
    }
    if get_boyia_class_id(value) != BuiltinId::kBoyiaArray.as_key() {
        return None;
    }
    let fun = (*value).mValue.mObj.mPtr as *const BoyiaFunction;
    if fun.is_null() {
        return Some(Vec::new());
    }
    if (*fun).mParams.is_null() {
        return Some(Vec::new());
    }
    let mut items = Vec::with_capacity((*fun).mParamSize as usize);
    for i in 0..(*fun).mParamSize {
        let prop = (*fun).mParams.add(i as usize);
        if matches!(
            (*prop).mValueType,
            ValueType::BY_NAV_FUNC | ValueType::BY_FUNC | ValueType::BY_PROP_FUNC
        ) {
            continue;
        }
        items.push(value_to_scalar(prop as *const BoyiaValue)?);
    }
    Some(items)
}

fn value_to_scalar(value: *const BoyiaValue) -> Option<BoyiaScalar> {
    if let Some(s) = value_to_string(value) {
        return Some(BoyiaScalar::Str(s));
    }
    if value.is_null() {
        return None;
    }
    let v = unsafe { &*value };
    match v.mValueType {
        ValueType::BY_INT | ValueType::BY_CHAR => Some(BoyiaScalar::Int(unsafe { v.mValue.mIntVal as i64 })),
        ValueType::BY_REAL => Some(BoyiaScalar::Int(unsafe { v.mValue.mRealVal as i64 })),
        _ => None,
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

/// Push an integer callback argument (for tuple-return sync builtins).
pub fn push_callback_int(value: i64, _vm: &mut BoyiaVM) -> Option<BoyiaValue> {
    Some(BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_INT,
        mValue: RealValue {
            mIntVal: value as LIntPtr,
        },
    })
}

/// Push a bool callback argument.
pub fn push_callback_bool(value: bool, vm: &mut BoyiaVM) -> Option<BoyiaValue> {
    push_callback_int(if value { 1 } else { 0 }, vm)
}

/// Push a float callback argument.
pub fn push_callback_f64(value: f64, _vm: &mut BoyiaVM) -> Option<BoyiaValue> {
    Some(BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_REAL,
        mValue: RealValue { mRealVal: value },
    })
}

/// Push a string callback argument.
pub fn push_callback_string(value: String, vm: &mut BoyiaVM) -> Option<BoyiaValue> {
    unsafe { string_to_boyia_value(vm, &value) }
}

#[macro_export]
macro_rules! define_sync_native {
    ($native:ident, $min:expr, $handler:ident) => {
        unsafe fn $native(vm: &mut boyia_vm::BoyiaVM) -> boyia_vm::OpHandleResult {
            $crate::runner::builtin_sync::sync_dispatch(vm, $min, $handler)
        }
    };
}
