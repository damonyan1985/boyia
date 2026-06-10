//! Json builtin: sync `parse` / `toString` and async `asyncParse` / `asyncToString`.

#![allow(dead_code)]

use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use serde_json::Value as JsonValue;

struct JsonBuiltins;

#[boyia_class(name = "Json", registrar = builtin_json_class)]
impl JsonBuiltins {
    #[boyia_sync_builtin(native = json_parse_native, method = "parse")]
    fn json_parse(text: String) -> Option<JsonValue> {
        serde_json::from_str(&text).ok()
    }

    #[boyia_sync_builtin(native = json_to_string_native, method = "toString")]
    fn json_to_string(value: JsonValue) -> String {
        serde_json::to_string(&value).unwrap_or_default()
    }

    #[boyia_async_builtin(native = json_async_parse_native, method = "asyncParse")]
    fn async_parse(text: String) -> AsyncBuiltinResult {
        match serde_json::from_str::<JsonValue>(&text) {
            Ok(value) => AsyncBuiltinResult::OkJson(value),
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("Json.asyncParse: {err}"),
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
                message: format!("Json.asyncToString: {err}"),
            },
        }
    }
}
