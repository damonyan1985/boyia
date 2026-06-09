//! Https builtin: requests on thread pool, callbacks on Boyia task thread.

#![allow(dead_code)]

use crate::runner::r#async::{
    attach_method, register_async_builtin_class, AsyncBuiltinResult, AsyncCtx, CallSite, ScriptCallback,
};
use crate::define_async_native;
use crate::some_or_end;
use boyia_vm::{LUintPtr, OpHandleResult, BoyiaVM};
use reqwest::blocking::Client;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::Value;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

pub fn builtin_https_class(vm: &mut BoyiaVM, gen_id: &mut dyn FnMut(&str) -> LUintPtr) {
    register_async_builtin_class(vm, gen_id, "Https", |class_body, vm, gen_id| {
        attach_method(gen_id, "load", https_load_native, class_body, vm);
        attach_method(gen_id, "request", https_request_native, class_body, vm);
    });
}

fn schedule_request(
    ctx: &AsyncCtx,
    url: String,
    params: Option<String>,
    callback: ScriptCallback,
) -> bool {
    ctx.spawn(
        move || match execute_https_request(&url, params.as_deref()) {
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
        },
        callback,
        |r| println!("https result: {}", r.log_preview()),
    )
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

fn https_load_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let url = some_or_end!(site.arg_string(1));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_request(site.ctx(), url, None, callback))
}

fn https_request_handler(site: &mut CallSite<'_>) -> OpHandleResult {
    let url = some_or_end!(site.arg_string(1));
    let params = some_or_end!(site.arg_string(2));
    let callback = some_or_end!(site.callback());
    site.finish(schedule_request(site.ctx(), url, Some(params), callback))
}

define_async_native!(https_load_native, 3, https_load_handler);
define_async_native!(https_request_native, 4, https_request_handler);
