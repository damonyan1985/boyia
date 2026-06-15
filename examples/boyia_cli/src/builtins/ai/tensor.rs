//! Tensor factory builtins, modeled on PyTorch (`empty` / `zeros` / `ones` / `full` / `tensor` / `arange` / `randn`).
//!
//! Script arrays map to Rust `Vec<usize>` (shape) or nested [`NestedVec`] (tensor data).

#![allow(dead_code)]

use builtin_macro::boyia_class;
use crate::runner::builtin_vec::NestedVec;
use rand::Rng;
use std::sync::{Mutex, OnceLock};

/// 1-based id; maps to [`TensorRegistry::slots`] at index `handle - 1` (`0` = invalid).
pub type Handle = usize;

/// Reserved handle: creation failed or invalid id.
pub const TENSOR_HANDLE_INVALID: Handle = 0;

fn slot_index(handle: Handle) -> Option<usize> {
    handle.checked_sub(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TensorDtype {
    Float32,
    Float64,
    Int64,
    Int32,
    Bool,
}

impl TensorDtype {
    fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "float32" | "f32" => Some(Self::Float32),
            "float64" | "f64" | "double" => Some(Self::Float64),
            "int64" | "i64" | "long" => Some(Self::Int64),
            "int32" | "i32" | "int" => Some(Self::Int32),
            "bool" | "boolean" => Some(Self::Bool),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Float32 => "float32",
            Self::Float64 => "float64",
            Self::Int64 => "int64",
            Self::Int32 => "int32",
            Self::Bool => "bool",
        }
    }

    fn element_size(&self) -> usize {
        match self {
            Self::Float32 => 4,
            Self::Float64 => 8,
            Self::Int64 => 8,
            Self::Int32 => 4,
            Self::Bool => 1,
        }
    }
}

/// Contiguous strided tensor storage (CPU). Mirrors PyTorch's shape + dtype + backing buffer.
#[derive(Clone, Debug)]
pub struct BoyiaTensor {
    pub shape: Vec<usize>,
    pub dtype: TensorDtype,
    pub storage: TensorStorage,
    pub requires_grad: bool,
}

#[derive(Clone, Debug)]
pub enum TensorStorage {
    F32(Vec<f32>),
    F64(Vec<f64>),
    I64(Vec<i64>),
    I32(Vec<i32>),
    Bool(Vec<bool>),
}

impl BoyiaTensor {
    pub fn numel(&self) -> usize {
        numel(&self.shape).unwrap_or(0)
    }

    fn repr(&self) -> String {
        let preview = self.storage.preview(10);
        if self.shape.is_empty() {
            format!("Tensor({}, dtype={})", preview, self.dtype.as_str())
        } else {
            format!(
                "Tensor(shape={:?}, dtype={}, data={})",
                self.shape,
                self.dtype.as_str(),
                preview
            )
        }
    }

    fn empty(shape: Vec<usize>, dtype: TensorDtype) -> Option<Self> {
        let n = numel(&shape)?;
        Some(Self {
            shape,
            dtype,
            storage: TensorStorage::zeroed(n, dtype),
            requires_grad: false,
        })
    }

    fn filled(shape: Vec<usize>, dtype: TensorDtype, fill: TensorScalar) -> Option<Self> {
        let n = numel(&shape)?;
        Some(Self {
            shape,
            dtype,
            storage: TensorStorage::filled(n, dtype, fill),
            requires_grad: false,
        })
    }

    fn from_nested(data: &[NestedVec], dtype: TensorDtype) -> Option<Self> {
        let (shape, scalars) = infer_from_nested(data)?;
        let n = numel(&shape)?;
        if scalars.len() != n {
            return None;
        }
        Some(Self {
            shape,
            dtype,
            storage: TensorStorage::from_scalars(scalars, dtype)?,
            requires_grad: false,
        })
    }

    fn arange(start: i64, end: i64, step: i64, dtype: TensorDtype) -> Option<Self> {
        if step == 0 {
            return None;
        }
        let mut values = Vec::new();
        if step > 0 {
            let mut cur = start;
            while cur < end {
                values.push(cur);
                cur += step;
            }
        } else {
            let mut cur = start;
            while cur > end {
                values.push(cur);
                cur += step;
            }
        }
        let n = values.len();
        Some(Self {
            shape: vec![n],
            dtype,
            storage: TensorStorage::from_i64_values(values, dtype)?,
            requires_grad: false,
        })
    }

    fn randn(shape: Vec<usize>, dtype: TensorDtype) -> Option<Self> {
        let n = numel(&shape)?;
        let mut rng = rand::rng();
        match dtype {
            TensorDtype::Float32 => {
                let data: Vec<f32> = (0..n)
                    .map(|_| {
                        let u1: f32 = rng.random_range(f32::MIN_POSITIVE..1.0);
                        let u2: f32 = rng.random_range(0.0..1.0);
                        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos()
                    })
                    .collect();
                Some(Self {
                    shape,
                    dtype,
                    storage: TensorStorage::F32(data),
                    requires_grad: false,
                })
            }
            TensorDtype::Float64 => {
                let data: Vec<f64> = (0..n)
                    .map(|_| {
                        let u1: f64 = rng.random_range(f64::MIN_POSITIVE..1.0);
                        let u2: f64 = rng.random_range(0.0..1.0);
                        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
                    })
                    .collect();
                Some(Self {
                    shape,
                    dtype,
                    storage: TensorStorage::F64(data),
                    requires_grad: false,
                })
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum TensorScalar {
    F64(f64),
}

impl TensorStorage {
    fn preview(&self, max_elems: usize) -> String {
        match self {
            Self::F32(v) => format_preview(v.iter().map(|x| format!("{x:.4}")), v.len(), max_elems),
            Self::F64(v) => format_preview(v.iter().map(|x| format!("{x:.4}")), v.len(), max_elems),
            Self::I64(v) => format_preview(v.iter().map(|x| x.to_string()), v.len(), max_elems),
            Self::I32(v) => format_preview(v.iter().map(|x| x.to_string()), v.len(), max_elems),
            Self::Bool(v) => format_preview(v.iter().map(|x| x.to_string()), v.len(), max_elems),
        }
    }

    fn zeroed(n: usize, dtype: TensorDtype) -> Self {
        match dtype {
            TensorDtype::Float32 => Self::F32(vec![0.0; n]),
            TensorDtype::Float64 => Self::F64(vec![0.0; n]),
            TensorDtype::Int64 => Self::I64(vec![0; n]),
            TensorDtype::Int32 => Self::I32(vec![0; n]),
            TensorDtype::Bool => Self::Bool(vec![false; n]),
        }
    }

    fn filled(n: usize, dtype: TensorDtype, fill: TensorScalar) -> Self {
        let TensorScalar::F64(v) = fill;
        match dtype {
            TensorDtype::Float32 => Self::F32(vec![v as f32; n]),
            TensorDtype::Float64 => Self::F64(vec![v; n]),
            TensorDtype::Int64 => Self::I64(vec![v as i64; n]),
            TensorDtype::Int32 => Self::I32(vec![v as i32; n]),
            TensorDtype::Bool => Self::Bool(vec![v != 0.0; n]),
        }
    }

    fn from_scalars(values: Vec<TensorScalar>, dtype: TensorDtype) -> Option<Self> {
        match dtype {
            TensorDtype::Float32 => Some(Self::F32(
                values
                    .into_iter()
                    .map(|TensorScalar::F64(v)| v as f32)
                    .collect(),
            )),
            TensorDtype::Float64 => Some(Self::F64(
                values.into_iter().map(|TensorScalar::F64(v)| v).collect(),
            )),
            TensorDtype::Int64 => Some(Self::I64(
                values
                    .into_iter()
                    .map(|TensorScalar::F64(v)| v as i64)
                    .collect(),
            )),
            TensorDtype::Int32 => Some(Self::I32(
                values
                    .into_iter()
                    .map(|TensorScalar::F64(v)| v as i32)
                    .collect(),
            )),
            TensorDtype::Bool => Some(Self::Bool(
                values
                    .into_iter()
                    .map(|TensorScalar::F64(v)| v != 0.0)
                    .collect(),
            )),
        }
    }

    fn from_i64_values(values: Vec<i64>, dtype: TensorDtype) -> Option<Self> {
        match dtype {
            TensorDtype::Float32 => Some(Self::F32(values.into_iter().map(|v| v as f32).collect())),
            TensorDtype::Float64 => Some(Self::F64(values.into_iter().map(|v| v as f64).collect())),
            TensorDtype::Int64 => Some(Self::I64(values)),
            TensorDtype::Int32 => Some(Self::I32(
                values.into_iter().map(|v| v as i32).collect(),
            )),
            TensorDtype::Bool => Some(Self::Bool(values.into_iter().map(|v| v != 0).collect())),
        }
    }
}

/// Handle-indexed tensor table: `handle - 1` is the index into [`TensorRegistry::slots`].
struct TensorRegistry {
    slots: Vec<Option<BoyiaTensor>>,
    /// Reusable 0-based slot indices (not handles).
    free_list: Vec<usize>,
}

impl TensorRegistry {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
        }
    }

    fn insert(&mut self, tensor: BoyiaTensor) -> Handle {
        if let Some(idx) = self.free_list.pop() {
            self.slots[idx] = Some(tensor);
            return idx + 1;
        }
        let idx = self.slots.len();
        self.slots.push(Some(tensor));
        idx + 1
    }

    fn get(&self, handle: Handle) -> Option<&BoyiaTensor> {
        let idx = slot_index(handle)?;
        self.slots.get(idx).and_then(|slot| slot.as_ref())
    }

    fn remove(&mut self, handle: Handle) -> bool {
        let idx = match slot_index(handle) {
            Some(i) => i,
            None => return false,
        };
        let Some(slot) = self.slots.get_mut(idx) else {
            return false;
        };
        if slot.take().is_some() {
            self.free_list.push(idx);
            true
        } else {
            false
        }
    }
}

fn registry() -> &'static Mutex<TensorRegistry> {
    static REG: OnceLock<Mutex<TensorRegistry>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(TensorRegistry::new()))
}

pub fn store_tensor(tensor: BoyiaTensor) -> Handle {
    let Ok(mut reg) = registry().lock() else {
        return TENSOR_HANDLE_INVALID;
    };
    reg.insert(tensor)
}

pub fn get_tensor(id: Handle) -> Option<BoyiaTensor> {
    let reg = registry().lock().ok()?;
    reg.get(id).cloned()
}

pub fn destroy_tensor(id: Handle) -> bool {
    let Ok(mut reg) = registry().lock() else {
        return false;
    };
    reg.remove(id)
}

fn parse_handle(id: i64) -> Option<Handle> {
    if id <= 0 {
        return None;
    }
    Some(id as Handle)
}

fn format_preview<I>(values: I, len: usize, max_elems: usize) -> String
where
    I: Iterator<Item = String>,
{
    if len == 0 {
        return "[]".into();
    }
    let mut out = String::from("[");
    for (i, s) in values.take(max_elems).enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push_str(&s);
    }
    if len > max_elems {
        out.push_str(", ...");
    }
    out.push(']');
    out
}

fn numel(shape: &[usize]) -> Option<usize> {
    if shape.is_empty() {
        return Some(1);
    }
    if shape.iter().any(|&d| d == 0) {
        return Some(0);
    }
    shape.iter().try_fold(1usize, |acc, &d| acc.checked_mul(d))
}

fn infer_from_nested(data: &[NestedVec]) -> Option<(Vec<usize>, Vec<TensorScalar>)> {
    if data.is_empty() {
        return Some((vec![0], Vec::new()));
    }
    if data.iter().all(|n| matches!(n, NestedVec::Item(_))) {
        let flat: Vec<TensorScalar> = data
            .iter()
            .map(|n| match n {
                NestedVec::Item(v) => TensorScalar::F64(*v),
                NestedVec::Items(_) => unreachable!(),
            })
            .collect();
        return Some((vec![data.len()], flat));
    }
    if data.iter().all(|n| matches!(n, NestedVec::Items(_))) {
        let mut inner_shapes = Vec::with_capacity(data.len());
        let mut rows: Vec<Vec<TensorScalar>> = Vec::with_capacity(data.len());
        for node in data {
            let NestedVec::Items(row) = node else { return None };
            let (shape, flat) = infer_from_nested(row)?;
            inner_shapes.push(shape);
            rows.push(flat);
        }
        if !inner_shapes.windows(2).all(|w| w[0] == w[1]) {
            return None;
        }
        let mut flat = Vec::new();
        for row in rows {
            flat.extend(row);
        }
        let mut shape = vec![data.len()];
        shape.extend(inner_shapes.first()?.clone());
        return Some((shape, flat));
    }
    None
}

fn parse_factory_args(shape: Vec<usize>, dtype: &str) -> Option<(Vec<usize>, TensorDtype)> {
    let dtype = TensorDtype::parse(dtype)?;
    Some((shape, dtype))
}

struct TensorBuiltins;

#[boyia_class(name = "Tensor", registrar = builtin_tensor_class)]
impl TensorBuiltins {
    #[boyia_sync_builtin(native = tensor_empty_native, method = "empty")]
    fn tensor_empty(
        shape: Vec<usize>,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some((shape, dtype)) = parse_factory_args(shape, &dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::empty(shape, dtype)
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_zeros_native, method = "zeros")]
    fn tensor_zeros(
        shape: Vec<usize>,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some((shape, dtype)) = parse_factory_args(shape, &dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::filled(shape, dtype, TensorScalar::F64(0.0))
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_ones_native, method = "ones")]
    fn tensor_ones(
        shape: Vec<usize>,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some((shape, dtype)) = parse_factory_args(shape, &dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::filled(shape, dtype, TensorScalar::F64(1.0))
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_full_native, method = "full")]
    fn tensor_full(
        shape: Vec<usize>,
        fill_value: f64,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some((shape, dtype)) = parse_factory_args(shape, &dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::filled(shape, dtype, TensorScalar::F64(fill_value))
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    /// `torch.tensor(data)` — `data` is `Vec<NestedVec>` (`[1,2,3]` or `[[1,2],[3,4]]`).
    #[boyia_sync_builtin(native = tensor_tensor_native, method = "tensor")]
    fn tensor_tensor(
        data: Vec<NestedVec>,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some(dtype) = TensorDtype::parse(&dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::from_nested(&data, dtype)
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_arange_native, method = "arange")]
    fn tensor_arange(
        end: i64,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some(dtype) = TensorDtype::parse(&dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::arange(0, end, 1, dtype)
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_arange_start_end_native, method = "arangeStartEnd")]
    fn tensor_arange_start_end(
        start: i64,
        end: i64,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some(dtype) = TensorDtype::parse(&dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::arange(start, end, 1, dtype)
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_arange_start_end_step_native, method = "arangeStartEndStep")]
    fn tensor_arange_start_end_step(
        start: i64,
        end: i64,
        step: i64,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some(dtype) = TensorDtype::parse(&dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::arange(start, end, step, dtype)
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_randn_native, method = "randn")]
    fn tensor_randn(
        shape: Vec<usize>,
        #[optional(default = "float32")]
        dtype: String,
    ) -> Handle {
        let Some((shape, dtype)) = parse_factory_args(shape, &dtype) else {
            return TENSOR_HANDLE_INVALID;
        };
        BoyiaTensor::randn(shape, dtype)
            .map(store_tensor)
            .unwrap_or(TENSOR_HANDLE_INVALID)
    }

    #[boyia_sync_builtin(native = tensor_shape_native, method = "shape")]
    fn tensor_shape(id: i64) -> Option<Vec<usize>> {
        let id = parse_handle(id)?;
        let reg = registry().lock().ok()?;
        reg.get(id).map(|t| t.shape.clone())
    }

    #[boyia_sync_builtin(native = tensor_to_string_native, method = "toString")]
    fn tensor_to_string(id: i64) -> String {
        let Some(id) = parse_handle(id) else {
            return String::new();
        };
        let Ok(reg) = registry().lock() else {
            return String::new();
        };
        reg.get(id)
            .map(|t| t.repr())
            .unwrap_or_default()
    }

    #[boyia_sync_builtin(native = tensor_destroy_native, method = "destroy")]
    fn tensor_destroy(id: i64) -> bool {
        parse_handle(id)
            .map(destroy_tensor)
            .unwrap_or(false)
    }
}
