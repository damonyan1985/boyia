//! CLI-specific builtin classes (File, Https, Zip).

pub mod file;
pub mod https;
pub mod zip;

use crate::runner::BuiltinRegistrar;

pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    https::builtin_https_class,
    file::builtin_file_class,
    zip::builtin_zip_class,
];