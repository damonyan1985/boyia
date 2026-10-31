//! Builtin class registrar function type (same as boyia_cli runner).

use boyia_vm::{BoyiaVM, LUintPtr};

pub type BuiltinRegistrar = fn(&mut BoyiaVM, &mut dyn FnMut(&str) -> LUintPtr);
