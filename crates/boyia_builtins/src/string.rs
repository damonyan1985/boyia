//! String builtin class: buffer, hash props; length, equal, indexOf, split, replace, substring, trim.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::gen_builtin_class_function;
use boyia_vm::{
    copy_object, create_global_class, create_native_string, create_string_object, get_boyia_class_id,
    get_function_count, get_local_size, get_local_value, get_string_buffer, get_string_hash,
    set_int_result, set_native_result, value_copy, BoyiaClass, BoyiaFunction, BoyiaStr, BoyiaValue,
    BuiltinId, K_BOYIA_NULL, NativePtr, RealValue, ValueType, LInt, LInt8, LIntPtr, LUintPtr,
    OpHandleResult, BoyiaVM,
};
use std::ptr;

fn str_eq(a: *const BoyiaStr, b: *const BoyiaStr) -> bool {
    if a.is_null() || b.is_null() {
        return a.is_null() && b.is_null();
    }
    let a = unsafe { &*a };
    let b = unsafe { &*b };
    if a.mLen != b.mLen {
        return false;
    }
    if a.mPtr == b.mPtr {
        return true;
    }
    let len = a.mLen.max(0) as usize;
    for i in 0..len {
        if unsafe { *a.mPtr.add(i) } != unsafe { *b.mPtr.add(i) } {
            return false;
        }
    }
    true
}

unsafe fn boyia_str_ptr_to_string(buf: *const BoyiaStr) -> String {
    if buf.is_null() {
        return String::new();
    }
    let len = (*buf).mLen.max(0) as usize;
    if len == 0 {
        return String::new();
    }
    let p = (*buf).mPtr as *const u8;
    if p.is_null() {
        return String::new();
    }
    let slice = std::slice::from_raw_parts(p, len);
    String::from_utf8_lossy(slice).into_owned()
}

unsafe fn value_to_string(val: *const BoyiaValue) -> Option<String> {
    if val.is_null() {
        return None;
    }
    match (*val).mValueType {
        ValueType::BY_STRING => Some(boyia_str_ptr_to_string(&(*val).mValue.mStrVal)),
        ValueType::BY_CLASS if get_boyia_class_id(val) == BuiltinId::kBoyiaString.as_key() => {
            Some(boyia_str_ptr_to_string(get_string_buffer(val)))
        }
        _ => None,
    }
}

unsafe fn string_to_boyia_value(vm: &mut BoyiaVM, s: &str) -> Option<BoyiaValue> {
    if s.is_empty() {
        let body = create_string_object(ptr::null_mut(), 0, vm);
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
    let buf = Box::into_raw(boxed) as *mut u8 as *mut LInt8;
    let mut value = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_INT,
        mValue: RealValue { mIntVal: 0 },
    };
    create_native_string(&mut value, buf, len, vm);
    if value.mValue.mObj.mPtr == K_BOYIA_NULL {
        return None;
    }
    value.mNameKey = BuiltinId::kBoyiaString.as_key();
    Some(value)
}

unsafe fn set_string_result(vm: &mut BoyiaVM, s: &str) -> OpHandleResult {
    match string_to_boyia_value(vm, s) {
        Some(mut value) => {
            set_native_result(&mut value, vm);
            OpHandleResult::kOpResultSuccess
        }
        None => OpHandleResult::kOpResultEnd,
    }
}

unsafe fn this_string(vm: &mut BoyiaVM) -> Option<String> {
    let size = get_local_size(vm);
    let obj = get_local_value(size - 1, vm) as *const BoyiaValue;
    value_to_string(obj)
}

unsafe fn string_length_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let obj = get_local_value(get_local_size(vm) - 1, vm) as *const BoyiaValue;
    let str_ref = get_string_buffer(obj);
    let len = if str_ref.is_null() {
        0
    } else {
        (*str_ref).mLen
    };
    set_int_result(len as LInt, vm);
    OpHandleResult::kOpResultSuccess
}

unsafe fn string_equal_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let size = get_local_size(vm);
    let obj = get_local_value(size - 1, vm) as *const BoyiaValue;
    let cmp_val = get_local_value(1, vm) as *const BoyiaValue;
    let str_a = get_string_buffer(obj);
    let str_b = get_string_buffer(cmp_val);
    let hash_a = get_string_hash(obj);
    let hash_b = get_string_hash(cmp_val);
    let eq = hash_a == hash_b && str_eq(str_a, str_b);
    set_int_result(if eq { 1 } else { 0 }, vm);
    OpHandleResult::kOpResultSuccess
}

/// indexOf(search[, from]): index of first match, or -1.
unsafe fn string_index_of_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let Some(haystack) = this_string(vm) else {
        return OpHandleResult::kOpResultEnd;
    };
    let search_val = get_local_value(1, vm) as *const BoyiaValue;
    let Some(needle) = value_to_string(search_val) else {
        return OpHandleResult::kOpResultEnd;
    };
    let from = if get_local_size(vm) > 2 {
        let from_val = get_local_value(2, vm) as *const BoyiaValue;
        if from_val.is_null() {
            0
        } else {
            (*from_val).mValue.mIntVal.max(0) as usize
        }
    } else {
        0
    };
    let idx = if from >= haystack.len() {
        None
    } else {
        haystack[from..].find(&needle)
    };
    let out = idx.map(|i| (from + i) as LInt).unwrap_or(-1);
    set_int_result(out, vm);
    OpHandleResult::kOpResultSuccess
}

/// split(delim): split into an Array of strings.
unsafe fn string_split_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let Some(haystack) = this_string(vm) else {
        return OpHandleResult::kOpResultEnd;
    };
    let delim_val = get_local_value(1, vm) as *const BoyiaValue;
    let Some(delim) = value_to_string(delim_val) else {
        return OpHandleResult::kOpResultEnd;
    };

    let parts: Vec<String> = if delim.is_empty() {
        haystack.chars().map(|c| c.to_string()).collect()
    } else {
        haystack.split(&delim).map(str::to_owned).collect()
    };

    let array_body =
        copy_object(BuiltinId::kBoyiaArray.as_key(), 32, vm) as *mut BoyiaFunction;
    if array_body.is_null() {
        return OpHandleResult::kOpResultEnd;
    }

    for part in parts {
        let Some(mut elem) = string_to_boyia_value(vm, &part) else {
            return OpHandleResult::kOpResultEnd;
        };
        elem.mNameKey = BuiltinId::kBoyiaString.as_key();
        if (*array_body).mParamSize >= get_function_count(array_body) {
            if !boyia_vm::vector_params_grow_if_full(array_body, vm) {
                return OpHandleResult::kOpResultEnd;
            }
        }
        let dst = (*array_body).mParams.add((*array_body).mParamSize as usize);
        value_copy(dst, &mut elem);
        (*array_body).mParamSize += 1;
    }

    let mut value = BoyiaValue {
        mNameKey: BuiltinId::kBoyiaArray.as_key(),
        mValueType: ValueType::BY_CLASS,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: array_body as LIntPtr,
                mSuper: K_BOYIA_NULL,
            },
        },
    };
    set_native_result(&mut value, vm);
    OpHandleResult::kOpResultSuccess
}

/// replace(search, replacement): replace all occurrences.
unsafe fn string_replace_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let Some(haystack) = this_string(vm) else {
        return OpHandleResult::kOpResultEnd;
    };
    let search_val = get_local_value(1, vm) as *const BoyiaValue;
    let replace_val = get_local_value(2, vm) as *const BoyiaValue;
    let Some(search) = value_to_string(search_val) else {
        return OpHandleResult::kOpResultEnd;
    };
    let Some(replacement) = value_to_string(replace_val) else {
        return OpHandleResult::kOpResultEnd;
    };
    set_string_result(vm, &haystack.replace(&search, &replacement))
}

/// substring(start[, end]): slice by byte indices (clamped).
unsafe fn string_substring_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let Some(haystack) = this_string(vm) else {
        return OpHandleResult::kOpResultEnd;
    };
    let start_val = get_local_value(1, vm) as *const BoyiaValue;
    if start_val.is_null() {
        return OpHandleResult::kOpResultEnd;
    }
    let mut start = (*start_val).mValue.mIntVal.max(0) as usize;
    let end = if get_local_size(vm) > 2 {
        let end_val = get_local_value(2, vm) as *const BoyiaValue;
        if end_val.is_null() {
            haystack.len()
        } else {
            (*end_val).mValue.mIntVal.max(0) as usize
        }
    } else {
        haystack.len()
    };
    if start > haystack.len() {
        start = haystack.len();
    }
    let end = end.min(haystack.len());
    let (lo, hi) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    set_string_result(vm, &haystack[lo..hi])
}

/// trim(): remove leading and trailing ASCII whitespace.
unsafe fn string_trim_impl(vm: &mut BoyiaVM) -> OpHandleResult {
    let Some(haystack) = this_string(vm) else {
        return OpHandleResult::kOpResultEnd;
    };
    set_string_result(vm, haystack.trim())
}

/// Register String builtin class: buffer, hash props; length, equal methods.
pub fn builtin_string_class<F>(vm: &mut BoyiaVM, gen_id: &mut F)
where
    F: FnMut(&str) -> LUintPtr,
{
    eprintln!("[builtin_string_class] 1");
    let string_key = gen_id("String");
    eprintln!("[builtin_string_class] 2 create_global_class string_key={}", string_key);
    let class_ref = unsafe { create_global_class(string_key, vm) } as *mut BoyiaValue;
    if class_ref.is_null() {
        eprintln!("[builtin_string_class] class_ref null");
        return;
    }
    eprintln!("[builtin_string_class] 3 class_ref ok");
    unsafe {
        (*class_ref).mValue.mObj.mSuper = K_BOYIA_NULL;
        let class_body = (*class_ref).mValue.mObj.mPtr as *mut BoyiaFunction;
        eprintln!("[builtin_string_class] 4 class_body={:?}", class_body);
        if class_body.is_null() || (*class_body).mParams.is_null() {
            eprintln!("[builtin_string_class] class_body or mParams null");
            return;
        }
        eprintln!("[builtin_string_class] 5 buffer prop");
        let buf_slot = (*class_body).mParams.add((*class_body).mParamSize as usize);
        (*buf_slot).mValueType = ValueType::BY_STRING;
        (*buf_slot).mNameKey = gen_id("buffer");
        (*buf_slot).mValue.mStrVal.mPtr = ptr::null_mut();
        (*buf_slot).mValue.mStrVal.mLen = 0;
        (*class_body).mParamSize += 1;
        eprintln!("[builtin_string_class] 6 hash prop");
        let hash_slot = (*class_body).mParams.add((*class_body).mParamSize as usize);
        (*hash_slot).mValueType = ValueType::BY_INT;
        (*hash_slot).mNameKey = gen_id("hash");
        (*hash_slot).mValue.mIntVal = 0;
        (*class_body).mParamSize += 1;
        eprintln!("[builtin_string_class] 7 length");
        gen_builtin_class_function(gen_id("length"), string_length_impl as NativePtr, class_body, vm);
        eprintln!("[builtin_string_class] 8 equal");
        gen_builtin_class_function(gen_id("equal"), string_equal_impl as NativePtr, class_body, vm);
        gen_builtin_class_function(gen_id("indexOf"), string_index_of_impl as NativePtr, class_body, vm);
        gen_builtin_class_function(gen_id("split"), string_split_impl as NativePtr, class_body, vm);
        gen_builtin_class_function(gen_id("replace"), string_replace_impl as NativePtr, class_body, vm);
        gen_builtin_class_function(gen_id("substring"), string_substring_impl as NativePtr, class_body, vm);
        gen_builtin_class_function(gen_id("trim"), string_trim_impl as NativePtr, class_body, vm);
    }
    eprintln!("[builtin_string_class] 9 done");
}
