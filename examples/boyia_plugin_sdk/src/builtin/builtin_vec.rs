//! Boyia Array → Rust `Vec` / nested [`NestedVec`] conversion for tensor builtins.

#![allow(non_snake_case)]

use boyia_vm::{
    copy_object, get_boyia_class_id, get_function_count, get_string_buffer, set_native_result,
    value_copy, vector_params_grow_if_full, BoyiaClass, BoyiaFunction, BoyiaValue, BuiltinId,
    K_BOYIA_NULL, LIntPtr, OpHandleResult, RealValue, ValueType, BoyiaVM,
};

/// Nested vector for tensor data: `Item` = scalar, `Items` = child dimension.
#[derive(Clone, Debug)]
pub enum NestedVec {
    Item(f64),
    Items(Vec<NestedVec>),
}

/// Write `Vec<usize>` as a Boyia Array into the sync native result slot (e.g. `Tensor.shape`).
pub fn set_sync_vec_usize_return(shape: Vec<usize>, vm: &mut BoyiaVM) -> OpHandleResult {
    unsafe {
        match vec_usize_to_boyia_array(vm, shape) {
            Ok(mut out) => {
                set_native_result(&mut out, vm);
                OpHandleResult::kOpResultSuccess
            }
            Err(_) => OpHandleResult::kOpResultEnd,
        }
    }
}

unsafe fn vec_usize_to_boyia_array(vm: &mut BoyiaVM, shape: Vec<usize>) -> Result<BoyiaValue, ()> {
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
    for d in shape {
        let elem = BoyiaValue {
            mNameKey: 0,
            mValueType: ValueType::BY_INT,
            mValue: RealValue {
                mIntVal: d as LIntPtr,
            },
        };
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
    if (*fun).mParams.is_null() {
        return Err(());
    }
    let dst = (*fun).mParams.add((*fun).mParamSize as usize);
    value_copy(dst, val);
    (*dst).mNameKey = 0;
    (*fun).mParamSize += 1;
    Ok(())
}

/// Read a 1-D Boyia Array of non-negative integers into `Vec<usize>` (tensor `shape`).
pub unsafe fn boyia_value_to_vec_usize(
    _vm: &mut BoyiaVM,
    v: *const BoyiaValue,
) -> Result<Vec<usize>, ()> {
    let items = read_array_elements(v)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(scalar_to_usize(item)?);
    }
    if out.iter().any(|&d| d == 0) {
        return Err(());
    }
    Ok(out)
}

/// Read nested Boyia Arrays into [`NestedVec`] tree (`[1,2]` or `[[1,2],[3,4]]`).
pub unsafe fn boyia_value_to_nested_vec(
    vm: &mut BoyiaVM,
    v: *const BoyiaValue,
) -> Result<Vec<NestedVec>, ()> {
    if v.is_null() {
        return Err(());
    }
    match (*v).mValueType {
        ValueType::BY_INT | ValueType::BY_CHAR | ValueType::BY_REAL => {
            Ok(vec![NestedVec::Item(scalar_to_f64(v)?)])
        }
        ValueType::BY_CLASS => {
            let cid = get_boyia_class_id(v);
            if cid == BuiltinId::kBoyiaArray.as_key() {
                let items = read_array_elements(v)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(nested_from_value(vm, item)?);
                }
                Ok(out)
            } else if cid == BuiltinId::kBoyiaString.as_key() {
                let s = boyia_str_to_string(v)?;
                let n: f64 = s.parse().map_err(|_| ())?;
                Ok(vec![NestedVec::Item(n)])
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

unsafe fn nested_from_value(vm: &mut BoyiaVM, v: *const BoyiaValue) -> Result<NestedVec, ()> {
    if v.is_null() {
        return Err(());
    }
    match (*v).mValueType {
        ValueType::BY_INT | ValueType::BY_CHAR | ValueType::BY_REAL => {
            Ok(NestedVec::Item(scalar_to_f64(v)?))
        }
        ValueType::BY_CLASS => {
            if get_boyia_class_id(v) == BuiltinId::kBoyiaArray.as_key() {
                let items = read_array_elements(v)?;
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(nested_from_value(vm, item)?);
                }
                Ok(NestedVec::Items(out))
            } else {
                Err(())
            }
        }
        _ => Err(()),
    }
}

unsafe fn read_array_elements(v: *const BoyiaValue) -> Result<Vec<*const BoyiaValue>, ()> {
    if v.is_null() || (*v).mValueType != ValueType::BY_CLASS {
        return Err(());
    }
    let fun = (*v).mValue.mObj.mPtr as *const BoyiaFunction;
    if fun.is_null() || (*fun).mParams.is_null() {
        return Err(());
    }
    let mut items = Vec::new();
    for i in 0..(*fun).mParamSize {
        let prop = (*fun).mParams.add(i as usize);
        if matches!(
            (*prop).mValueType,
            ValueType::BY_NAV_FUNC | ValueType::BY_FUNC | ValueType::BY_PROP_FUNC
        ) {
            continue;
        }
        items.push(prop as *const BoyiaValue);
    }
    Ok(items)
}

unsafe fn scalar_to_f64(v: *const BoyiaValue) -> Result<f64, ()> {
    if v.is_null() {
        return Err(());
    }
    Ok(match (*v).mValueType {
        ValueType::BY_INT => {
            if (*v).mValue.mIntVal == K_BOYIA_NULL {
                return Err(());
            }
            (*v).mValue.mIntVal as f64
        }
        ValueType::BY_CHAR => (*v).mValue.mIntVal as f64,
        ValueType::BY_REAL => (*v).mValue.mRealVal,
        ValueType::BY_CLASS if get_boyia_class_id(v) == BuiltinId::kBoyiaString.as_key() => {
            boyia_str_to_string(v)?.parse().map_err(|_| ())?
        }
        _ => return Err(()),
    })
}

unsafe fn scalar_to_usize(v: *const BoyiaValue) -> Result<usize, ()> {
    let n = scalar_to_f64(v)?;
    if n < 0.0 || n.fract() != 0.0 {
        return Err(());
    }
    Ok(n as usize)
}

unsafe fn boyia_str_to_string(v: *const BoyiaValue) -> Result<String, ()> {
    let buf = get_string_buffer(v);
    if buf.is_null() {
        return Err(());
    }
    let len = (*buf).mLen.max(0) as usize;
    let ptr = (*buf).mPtr as *const u8;
    let slice = std::slice::from_raw_parts(ptr, len);
    std::str::from_utf8(slice).map(|s| s.to_owned()).map_err(|_| ())
}
