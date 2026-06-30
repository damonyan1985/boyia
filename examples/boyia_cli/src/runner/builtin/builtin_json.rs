//! Boyia value ↔ JSON conversion helpers for sync/async Json builtins.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use boyia_vm::{
    copy_object, create_string_object, gen_identifier_from_str, get_boyia_class_id, get_function_count,
    get_string_buffer, name_for_identifier, set_native_result, value_copy, vector_params_grow_if_full,
    BoyiaClass, BoyiaFunction, BoyiaStr, BoyiaValue, BuiltinId, K_BOYIA_NULL, LInt, LInt8, LIntPtr,
    LVoid, OpHandleResult, RealValue, ValueType, BoyiaVM,
};
use serde_json::{Map as JsonMap, Number, Value as JsonValue};

unsafe fn boyia_str_to_slice<'a>(v: *const BoyiaValue) -> Option<&'a [u8]> {
    if v.is_null() {
        return None;
    }
    let buf = get_string_buffer(v);
    if buf.is_null() {
        return None;
    }
    let len = (*buf).mLen.max(0) as usize;
    let ptr = (*buf).mPtr as *const u8;
    Some(std::slice::from_raw_parts(ptr, len))
}

unsafe fn alloc_string_value(vm: &mut BoyiaVM, s: &str) -> Option<BoyiaValue> {
    let rt = boyia_vm::get_runtime_from_vm(vm);
    if rt.is_null() {
        return None;
    }
    let bytes = s.as_bytes();
    let len = bytes.len() as LInt;
    if len <= 0 {
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
    let p = (*rt).new_data(len) as *mut u8;
    if p.is_null() {
        return None;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p, bytes.len());
    let body = create_string_object(p as *mut LInt8, len, vm);
    if body.is_null() {
        (*rt).delete_data(p as *mut LVoid);
        return None;
    }
    Some(BoyiaValue {
        mNameKey: BuiltinId::kBoyiaString.as_key(),
        mValueType: ValueType::BY_CLASS,
        mValue: RealValue {
            mObj: BoyiaClass {
                mPtr: body as LIntPtr,
                mSuper: K_BOYIA_NULL,
            },
        },
    })
}

unsafe fn map_put(vm: &mut BoyiaVM, map_obj: *mut BoyiaValue, key: &str, val: &BoyiaValue) -> Result<(), ()> {
    let fun = (*map_obj).mValue.mObj.mPtr as *mut BoyiaFunction;
    if fun.is_null() || (*fun).mParams.is_null() {
        return Err(());
    }
    let kb = key.as_bytes();
    let bstr = BoyiaStr {
        mPtr: kb.as_ptr() as *mut LInt8,
        mLen: kb.len() as LInt,
    };
    let key_id = gen_identifier_from_str(vm, &bstr);
    let cap = get_function_count(fun);
    if (*fun).mParamSize >= cap && !vector_params_grow_if_full(fun, vm) {
        return Err(());
    }
    let slot = (*fun).mParams.add((*fun).mParamSize as usize);
    value_copy(slot, val);
    (*slot).mNameKey = key_id;
    (*fun).mParamSize += 1;
    Ok(())
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
    if (*fun).mParams.is_null() {
        return Err(());
    }
    let dst = (*fun).mParams.add((*fun).mParamSize as usize);
    value_copy(dst, val);
    (*dst).mNameKey = 0;
    (*fun).mParamSize += 1;
    Ok(())
}

unsafe fn serde_to_boyia(vm: &mut BoyiaVM, j: &JsonValue) -> Result<BoyiaValue, ()> {
    Ok(match j {
        JsonValue::Null => BoyiaValue {
            mNameKey: 0,
            mValueType: ValueType::BY_INT,
            mValue: RealValue {
                mIntVal: K_BOYIA_NULL,
            },
        },
        JsonValue::Bool(b) => BoyiaValue {
            mNameKey: 0,
            mValueType: ValueType::BY_INT,
            mValue: RealValue {
                mIntVal: if *b { 1 } else { 0 },
            },
        },
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i >= isize::MIN as i64 && i <= isize::MAX as i64 {
                    BoyiaValue {
                        mNameKey: 0,
                        mValueType: ValueType::BY_INT,
                        mValue: RealValue {
                            mIntVal: i as LIntPtr,
                        },
                    }
                } else {
                    BoyiaValue {
                        mNameKey: 0,
                        mValueType: ValueType::BY_REAL,
                        mValue: RealValue {
                            mRealVal: n.as_f64().unwrap_or(0.0),
                        },
                    }
                }
            } else {
                BoyiaValue {
                    mNameKey: 0,
                    mValueType: ValueType::BY_REAL,
                    mValue: RealValue {
                        mRealVal: n.as_f64().unwrap_or(0.0),
                    },
                }
            }
        }
        JsonValue::String(s) => alloc_string_value(vm, s).ok_or(())?,
        JsonValue::Array(a) => {
            let map_key = BuiltinId::kBoyiaArray.as_key();
            let raw = copy_object(map_key, 32, vm);
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
            for item in a {
                let elem = serde_to_boyia(vm, item)?;
                array_add(vm, &mut out, &elem)?;
            }
            out
        }
        JsonValue::Object(o) => {
            let map_key = BuiltinId::kBoyiaMap.as_key();
            let raw = copy_object(map_key, 32, vm);
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
            for (k, v) in o {
                let elem = serde_to_boyia(vm, v)?;
                map_put(vm, &mut out, k, &elem)?;
            }
            out
        }
    })
}

fn json_number_from_int(i: LIntPtr) -> JsonValue {
    JsonValue::Number(Number::from(i))
}

fn json_number_from_real(f: f64) -> JsonValue {
    JsonValue::Number(Number::from_f64(f).unwrap_or_else(|| Number::from(0)))
}

/// Convert a Boyia value (Map / Array / String / number / null) to [JsonValue].
pub unsafe fn boyia_value_to_json(vm: &mut BoyiaVM, v: *const BoyiaValue) -> Result<JsonValue, ()> {
    boyia_to_serde(vm, v)
}

/// Convert [JsonValue] into a Boyia value.
pub unsafe fn json_to_boyia_value(vm: &mut BoyiaVM, j: &JsonValue) -> Result<BoyiaValue, ()> {
    serde_to_boyia(vm, j)
}

/// Set sync `parse` native result from parsed JSON.
pub fn set_sync_json_return(j: JsonValue, vm: &mut BoyiaVM) -> OpHandleResult {
    unsafe {
        match json_to_boyia_value(vm, &j) {
            Ok(mut out) => {
                set_native_result(&mut out, vm);
                OpHandleResult::kOpResultSuccess
            }
            Err(_) => OpHandleResult::kOpResultEnd,
        }
    }
}

unsafe fn boyia_to_serde(vm: &mut BoyiaVM, v: *const BoyiaValue) -> Result<JsonValue, ()> {
    if v.is_null() {
        return Err(());
    }
    match (*v).mValueType {
        ValueType::BY_INT => {
            if (*v).mValue.mIntVal == K_BOYIA_NULL {
                Ok(JsonValue::Null)
            } else {
                Ok(json_number_from_int((*v).mValue.mIntVal))
            }
        }
        ValueType::BY_REAL => Ok(json_number_from_real((*v).mValue.mRealVal)),
        ValueType::BY_CLASS => {
            let cid = get_boyia_class_id(v);
            if cid == BuiltinId::kBoyiaString.as_key() {
                let slice = boyia_str_to_slice(v).ok_or(())?;
                let s = std::str::from_utf8(slice).map_err(|_| ())?;
                Ok(JsonValue::String(s.to_string()))
            } else if cid == BuiltinId::kBoyiaArray.as_key() {
                let fun = (*v).mValue.mObj.mPtr as *const BoyiaFunction;
                if fun.is_null() || (*fun).mParams.is_null() {
                    return Err(());
                }
                let mut vec = Vec::new();
                for i in 0..(*fun).mParamSize {
                    let prop = (*fun).mParams.add(i as usize);
                    if matches!(
                        (*prop).mValueType,
                        ValueType::BY_NAV_FUNC | ValueType::BY_FUNC | ValueType::BY_PROP_FUNC
                    ) {
                        continue;
                    }
                    vec.push(boyia_to_serde(vm, prop)?);
                }
                Ok(JsonValue::Array(vec))
            } else if cid == BuiltinId::kBoyiaMap.as_key() {
                let fun = (*v).mValue.mObj.mPtr as *const BoyiaFunction;
                if fun.is_null() || (*fun).mParams.is_null() {
                    return Err(());
                }
                let mut m = JsonMap::new();
                for i in 0..(*fun).mParamSize {
                    let prop = (*fun).mParams.add(i as usize);
                    if matches!(
                        (*prop).mValueType,
                        ValueType::BY_NAV_FUNC | ValueType::BY_FUNC | ValueType::BY_PROP_FUNC
                    ) {
                        continue;
                    }
                    let key = name_for_identifier(vm, (*prop).mNameKey).ok_or(())?;
                    m.insert(key, boyia_to_serde(vm, prop)?);
                }
                Ok(JsonValue::Object(m))
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}
