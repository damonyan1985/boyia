//! JsonBuiltin: async JSON parse / stringify on the thread pool.

#![allow(dead_code)]

use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use serde_json::Value as JsonValue;

struct JsonBuiltin;

#[boyia_class(name = "JsonBuiltin", registrar = builtin_json_builtin_class)]
impl JsonBuiltin {
    #[boyia_async_builtin(native = json_async_parse_native, method = "asyncParse")]
    fn async_parse(text: String) -> AsyncBuiltinResult {
        match serde_json::from_str::<JsonValue>(&text) {
            Ok(value) => AsyncBuiltinResult::OkJson(value),
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("JsonBuiltin.asyncParse: {err}"),
            },
        }
    }

    #[boyia_async_builtin(native = json_async_to_string_native, method = "asyncToString")]
    fn async_to_string(value: JsonValue) -> AsyncBuiltinResult {
        match serde_json::to_string(&value) {
            Ok(text) => AsyncBuiltinResult::Ok {
                data: Some(text),
            },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("JsonBuiltin.asyncToString: {err}"),
            },
        }
    }
}
