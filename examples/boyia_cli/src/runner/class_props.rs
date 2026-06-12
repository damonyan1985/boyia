//! Boyia class `mParams` property slots: attach at register time, load/store in native handlers.

use boyia_vm::{
    create_native_string, get_string_buffer, BoyiaFunction, BoyiaValue, RealValue, ValueType,
    LInt, LInt8, LIntPtr, LUintPtr, BoyiaVM,
};

/// Append a `bool` property slot to a builtin class body (`BY_INT`: 0 / 1).
pub unsafe fn attach_class_prop_bool(
    class_body: *mut BoyiaFunction,
    key: LUintPtr,
    default: bool,
) {
    if class_body.is_null() || (*class_body).mParams.is_null() {
        return;
    }
    let slot = (*class_body).mParams.add((*class_body).mParamSize as usize);
    (*slot).mNameKey = key;
    (*slot).mValueType = ValueType::BY_INT;
    (*slot).mValue.mIntVal = if default { 1 } else { 0 };
    (*class_body).mParamSize += 1;
}

/// Append an integer property slot (`BY_INT`).
pub unsafe fn attach_class_prop_i64(
    class_body: *mut BoyiaFunction,
    key: LUintPtr,
    default: i64,
) {
    if class_body.is_null() || (*class_body).mParams.is_null() {
        return;
    }
    let slot = (*class_body).mParams.add((*class_body).mParamSize as usize);
    (*slot).mNameKey = key;
    (*slot).mValueType = ValueType::BY_INT;
    (*slot).mValue.mIntVal = default as LIntPtr;
    (*class_body).mParamSize += 1;
}

/// Append a float property slot (`BY_REAL`).
pub unsafe fn attach_class_prop_f64(
    class_body: *mut BoyiaFunction,
    key: LUintPtr,
    default: f64,
) {
    if class_body.is_null() || (*class_body).mParams.is_null() {
        return;
    }
    let slot = (*class_body).mParams.add((*class_body).mParamSize as usize);
    (*slot).mNameKey = key;
    (*slot).mValueType = ValueType::BY_REAL;
    (*slot).mValue.mRealVal = default;
    (*class_body).mParamSize += 1;
}

/// Append an empty native string property (`BY_STRING`).
pub unsafe fn attach_class_prop_string(
    class_body: *mut BoyiaFunction,
    key: LUintPtr,
    default: &str,
    vm: &mut BoyiaVM,
) {
    if class_body.is_null() || (*class_body).mParams.is_null() {
        return;
    }
    let slot = (*class_body).mParams.add((*class_body).mParamSize as usize);
    (*slot).mNameKey = key;
    (*slot).mValueType = ValueType::BY_STRING;
    if default.is_empty() {
        (*slot).mValue.mStrVal.mPtr = std::ptr::null_mut();
        (*slot).mValue.mStrVal.mLen = 0;
    } else if !write_string_slot(slot, default, vm) {
        (*slot).mValue.mStrVal.mPtr = std::ptr::null_mut();
        (*slot).mValue.mStrVal.mLen = 0;
    }
    (*class_body).mParamSize += 1;
}

pub unsafe fn prop_load_bool(class_body: *mut BoyiaFunction, index: usize) -> bool {
    let slot = (*class_body).mParams.add(index);
    (*slot).mValue.mIntVal != 0
}

pub unsafe fn prop_store_bool(class_body: *mut BoyiaFunction, index: usize, value: bool) {
    let slot = (*class_body).mParams.add(index);
    (*slot).mValueType = ValueType::BY_INT;
    (*slot).mValue.mIntVal = if value { 1 } else { 0 };
}

pub unsafe fn prop_load_i64(class_body: *mut BoyiaFunction, index: usize) -> i64 {
    let slot = (*class_body).mParams.add(index);
    (*slot).mValue.mIntVal as i64
}

pub unsafe fn prop_store_i64(class_body: *mut BoyiaFunction, index: usize, value: i64) {
    let slot = (*class_body).mParams.add(index);
    (*slot).mValueType = ValueType::BY_INT;
    (*slot).mValue.mIntVal = value as LIntPtr;
}

pub unsafe fn prop_load_u64(class_body: *mut BoyiaFunction, index: usize) -> u64 {
    prop_load_i64(class_body, index) as u64
}

pub unsafe fn prop_store_u64(class_body: *mut BoyiaFunction, index: usize, value: u64) {
    prop_store_i64(class_body, index, value as i64);
}

pub unsafe fn prop_load_f64(class_body: *mut BoyiaFunction, index: usize) -> f64 {
    let slot = (*class_body).mParams.add(index);
    match (*slot).mValueType {
        ValueType::BY_REAL => (*slot).mValue.mRealVal,
        ValueType::BY_INT | ValueType::BY_CHAR => (*slot).mValue.mIntVal as f64,
        _ => 0.0,
    }
}

pub unsafe fn prop_store_f64(class_body: *mut BoyiaFunction, index: usize, value: f64) {
    let slot = (*class_body).mParams.add(index);
    (*slot).mValueType = ValueType::BY_REAL;
    (*slot).mValue.mRealVal = value;
}

pub unsafe fn prop_load_string(class_body: *mut BoyiaFunction, index: usize) -> String {
    let slot = (*class_body).mParams.add(index);
    let val = slot as *const BoyiaValue;
    value_to_string(val).unwrap_or_default()
}

pub unsafe fn prop_store_string(
    class_body: *mut BoyiaFunction,
    index: usize,
    value: &str,
    vm: &mut BoyiaVM,
) {
    let slot = (*class_body).mParams.add(index);
    (*slot).mValueType = ValueType::BY_STRING;
    if value.is_empty() {
        (*slot).mValue.mStrVal.mPtr = std::ptr::null_mut();
        (*slot).mValue.mStrVal.mLen = 0;
        return;
    }
    let _ = write_string_slot(slot, value, vm);
}

unsafe fn write_string_slot(slot: *mut BoyiaValue, s: &str, vm: &mut BoyiaVM) -> bool {
    let boxed = s.as_bytes().to_vec().into_boxed_slice();
    let len = boxed.len() as LInt;
    let ptr = Box::into_raw(boxed) as *mut u8 as *mut LInt8;
    let mut tmp = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_INT,
        mValue: RealValue { mIntVal: 0 },
    };
    create_native_string(&mut tmp, ptr, len, vm);
    if tmp.mValue.mObj.mPtr == boyia_vm::K_BOYIA_NULL {
        return false;
    }
    let bstr = get_string_buffer(&tmp as *const BoyiaValue);
    if bstr.is_null() {
        return false;
    }
    (*slot).mValue.mStrVal = *bstr;
    true
}

fn value_to_string(value: *const BoyiaValue) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let str_ref = unsafe { get_string_buffer(value) };
    if str_ref.is_null() {
        return None;
    }
    let len = unsafe { (*str_ref).mLen.max(0) as usize };
    let ptr = unsafe { (*str_ref).mPtr as *const u8 };
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    std::str::from_utf8(slice).ok().map(ToOwned::to_owned)
}
