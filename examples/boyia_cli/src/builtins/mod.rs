//! CLI-specific builtin classes (File, Https, Zip, Json, Tensor).

pub mod ai;
pub mod external;
pub mod utility;

use crate::runner::BuiltinRegistrar;

pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    external::config::builtin_config_class,
    external::hashmap::builtin_hashmap_class,
    external::ws_server::builtin_websocket_server_class,
    utility::app::builtin_app_class,
    utility::https::builtin_https_class,
    utility::file::builtin_file_class,
    utility::os::builtin_os_class,
    utility::zip::builtin_zip_class,
    utility::json::builtin_json_class,
    ai::tensor::builtin_tensor_class,
];