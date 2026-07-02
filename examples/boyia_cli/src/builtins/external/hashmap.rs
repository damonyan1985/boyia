//! HashMap builtin: keys are integers or strings; values may be int, float, string, or array.

#![allow(dead_code)]

use builtin_macro::{boyia_class, boyia_native_object};
use crate::runner::builtin_sync::BoyiaScalar;
use std::collections::HashMap as StdHashMap;

#[boyia_native_object]
pub struct HashMapBuiltins {
    #[boyia_field(skip, init = "StdHashMap::new()")]
    entries: StdHashMap<BoyiaScalar, BoyiaScalar>,
}

#[boyia_class(name = "HashMap", registrar = builtin_hashmap_class)]
impl HashMapBuiltins {
    /// Insert or update `key` with `value`.
    #[boyia_sync_builtin(method = "put")]
    fn put(&mut self, key: BoyiaScalar, value: BoyiaScalar) -> bool {
        self.entries.insert(key, value);
        true
    }

    /// Return the value for `key`; missing int keys default to `0`, string keys to `""`.
    #[boyia_sync_builtin(method = "get")]
    fn get(&self, key: BoyiaScalar) -> BoyiaScalar {
        self.entries
            .get(&key)
            .cloned()
            .unwrap_or_else(|| BoyiaScalar::missing_default_for_key(&key))
    }

    /// Return the value for `key`, or `default` when missing.
    #[boyia_sync_builtin(method = "getOr")]
    fn get_or(&self, key: BoyiaScalar, default: BoyiaScalar) -> BoyiaScalar {
        self.entries.get(&key).cloned().unwrap_or(default)
    }

    /// Remove `key`; returns true when the key existed.
    #[boyia_sync_builtin(method = "remove")]
    fn remove(&mut self, key: BoyiaScalar) -> bool {
        self.entries.remove(&key).is_some()
    }

    /// Clear all entries.
    #[boyia_sync_builtin(method = "clear")]
    fn clear(&mut self) -> bool {
        self.entries.clear();
        true
    }

    /// Return whether `key` exists.
    #[boyia_sync_builtin(method = "has")]
    fn has(&self, key: BoyiaScalar) -> bool {
        self.entries.contains_key(&key)
    }

    /// Number of entries.
    #[boyia_sync_builtin(method = "size")]
    fn size(&self) -> u64 {
        self.entries.len() as u64
    }

    /// All keys as a Boyia Array (each element is int or string; keys are never float).
    #[boyia_sync_builtin(method = "keys")]
    fn keys(&self) -> Vec<BoyiaScalar> {
        self.entries.keys().cloned().collect()
    }

    /// All values as a Boyia Array (each element is int, float, string, or array).
    #[boyia_sync_builtin(method = "values")]
    fn values(&self) -> Vec<BoyiaScalar> {
        self.entries.values().cloned().collect()
    }
}
