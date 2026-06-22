//! Https builtin: requests on thread pool, callbacks on Boyia task thread.

#![allow(dead_code)]

use crate::runner::builtin_async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

fn log_https_result(r: &AsyncBuiltinResult) {
    println!("https result: {}", r.log_preview());
}

fn execute_https_request(url: &str, params: Option<&str>) -> Result<String, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .build()
        .map_err(|err| err.to_string())?;

    let mut method = "get".to_string();
    let mut headers = HeaderMap::new();
    let mut body = None::<String>;

    if let Some(params) = params {
        let json: Value = serde_json::from_str(params).map_err(|err| err.to_string())?;

        if let Some(method_value) = json.get("method").and_then(Value::as_str) {
            method = method_value.to_ascii_lowercase();
        }

        if let Some(header_obj) = json.get("headers").and_then(Value::as_object) {
            for (name, value) in header_obj {
                let Some(value) = value.as_str() else {
                    continue;
                };
                let header_name =
                    HeaderName::from_bytes(name.as_bytes()).map_err(|err| err.to_string())?;
                let header_value = HeaderValue::from_str(value).map_err(|err| err.to_string())?;
                headers.insert(header_name, header_value);
            }
        }

        body = json
            .get("body")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }

    let mut request = match method.as_str() {
        "post" => client.post(url),
        _ => client.get(url),
    };

    if !headers.is_empty() {
        request = request.headers(headers);
    }

    if let Some(body) = body {
        request = request.body(body);
    }

    request
        .send()
        .map_err(|err| err.to_string())?
        .text()
        .map_err(|err| err.to_string())
}

fn https_result(url: &str, params: Option<&str>) -> AsyncBuiltinResult {
    match execute_https_request(url, params) {
        Ok(text) => {
            if text.is_empty() {
                AsyncBuiltinResult::Ok { data: None }
            } else {
                AsyncBuiltinResult::Ok {
                    data: Some(text),
                }
            }
        }
        Err(err) => AsyncBuiltinResult::Fail {
            message: format!("Https error: {err}"),
        },
    }
}

struct HttpsBuiltins;

#[boyia_class(name = "Https", registrar = builtin_https_class)]
impl HttpsBuiltins {
    #[boyia_async_builtin(method = "load")]
    fn https_load(url: String) -> AsyncBuiltinResult {
        https_result(&url, None)
    }

    #[boyia_async_builtin(method = "request", before = log_https_result)]
    fn https_request(url: String, params: String) -> AsyncBuiltinResult {
        https_result(&url, Some(params.as_str()))
    }
}
