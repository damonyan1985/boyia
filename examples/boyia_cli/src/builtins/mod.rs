//! CLI-specific builtin classes (File, Https, Zip, Json, Tensor).

pub mod ai;
pub mod utility;

use crate::runner::BuiltinRegistrar;

pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    utility::https::builtin_https_class,
    utility::file::builtin_file_class,
    utility::zip::builtin_zip_class,
    utility::json::builtin_json_class,
    ai::tensor::builtin_tensor_class,
];