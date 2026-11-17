//! 全局查找未命中钩子：宿主可在首次解析未知全局标识符时按需加载插件并注册 builtin。
//!
//! 钩子返回注册后查到的全局值（`null` 表示仍未命中）。调用方只查一次、不重试。

use crate::types::{BoyiaValue, BoyiaVM, LUintPtr};
use std::sync::OnceLock;

pub type GlobalMissHook = fn(&mut BoyiaVM, LUintPtr) -> *mut BoyiaValue;

static GLOBAL_MISS_HOOK: OnceLock<GlobalMissHook> = OnceLock::new();

/// 安装进程级全局未命中钩子。只写一次；已安装时返回 `false`。
pub fn set_global_miss_hook(hook: GlobalMissHook) -> bool {
    GLOBAL_MISS_HOOK.set(hook).is_ok()
}

/// 触发钩子（若已安装）。
pub(crate) fn resolve_global_miss(vm: &mut BoyiaVM, key: LUintPtr) -> *mut BoyiaValue {
    match GLOBAL_MISS_HOOK.get() {
        Some(hook) => hook(vm, key),
        None => std::ptr::null_mut(),
    }
}
