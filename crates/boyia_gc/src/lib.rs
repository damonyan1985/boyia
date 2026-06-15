//! Boyia GC. Public API only; implementation in gc.rs.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

mod gc;
mod native_gc;

pub use gc::{BoyiaGc, create_gc, destroy_gc, gc_append_ref, gc_collect_garbage};
pub use native_gc::{
    attach_native_ptr_slot, boyia_ensure_native, boyia_native_mut, boyia_native_ref, drop_native,
    flag as native_object_flag, get_native_ptr, header as native_gc_header,
    mark as mark_native_object, set_native_ptr, NativePropHeader, NativePropTrait, NativePropVTable,
    BOYIA_NATIVE_PTR_NAME, BOYIA_NATIVE_PTR_SLOT, K_NATIVE_GC_BLACK, K_NATIVE_GC_WHITE,
};
