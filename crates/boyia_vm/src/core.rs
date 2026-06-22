//! Boyia VM core implementation
//! All main VM logic lives here; no C/FFI surface.

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

use crate::types::*;
use crate::Runtime;
use boyia_memory::{
    alloc_memory_chunk, create_memory_cache, destroy_memory_cache, free_memory_chunk,
};
use std::ptr;
use std::alloc::{alloc_zeroed, dealloc, Layout};
use std::mem;

// ---------------------------------------------------------------------------
// Function/array capacity (GET_FUNCTION_COUNT)
// ---------------------------------------------------------------------------

/// Capacity from function/array body (mParamCount & 0x0000FFFF). Match GET_FUNCTION_COUNT in C++.
#[inline]
pub unsafe fn get_function_count(fun: *const BoyiaFunction) -> LInt {
    if fun.is_null() {
        return 0;
    }
    (*fun).mParamCount & 0x0000_FFFF
}

// ---------------------------------------------------------------------------
// Value copy
// ---------------------------------------------------------------------------

/// Copy value without name field (internal).
pub(crate) unsafe fn value_copy_no_name(dest: *mut BoyiaValue, src: *const BoyiaValue) {
    if dest.is_null() || src.is_null() {
        return;
    }
    (*dest).mValueType = (*src).mValueType;
    let t = (*src).mValueType;
    if t == ValueType::BY_INT || t == ValueType::BY_CHAR || t == ValueType::BY_NAVCLASS {
        (*dest).mValue.mIntVal = (*src).mValue.mIntVal;
    } else if t == ValueType::BY_REAL {
        (*dest).mValue.mRealVal = (*src).mValue.mRealVal;
    } else if t == ValueType::BY_FUNC {
        (*dest).mValue.mObj.mPtr = (*src).mValue.mObj.mPtr;
    } else if t == ValueType::BY_PROP_FUNC
        || t == ValueType::BY_ASYNC_PROP
        || t == ValueType::BY_NAV_PROP
        || t == ValueType::BY_ANONYM_FUNC
        || t == ValueType::BY_CLASS
    {
        (*dest).mValue.mObj.mPtr = (*src).mValue.mObj.mPtr;
        (*dest).mValue.mObj.mSuper = (*src).mValue.mObj.mSuper;
    } else if t == ValueType::BY_STRING {
        (*dest).mValue.mStrVal = (*src).mValue.mStrVal;
    } else {
        (*dest).mValue = (*src).mValue;
    }
}

/// Copy value (name + value).
pub unsafe fn value_copy(dest: *mut BoyiaValue, src: *const BoyiaValue) {
    if dest.is_null() || src.is_null() {
        return;
    }
    (*dest).mNameKey = (*src).mNameKey;
    value_copy_no_name(dest, src);
}

/// Match `BoyiaCore.cpp` `ValueCopyWithKey`: `dest->mNameKey = (LUintPtr)src; ValueCopyNoName(dest, src)`.
/// Stores the **address** of `src` in `dest.mNameKey` (for indirection / property slots), not `src.mNameKey`.
#[inline]
pub(crate) unsafe fn value_copy_with_key(dest: *mut BoyiaValue, src: *const BoyiaValue) {
    if dest.is_null() || src.is_null() {
        return;
    }
    (*dest).mNameKey = src as LUintPtr;
    value_copy_no_name(dest, src);
}

// ---------------------------------------------------------------------------
// Native result, locals, stack, global table
// ---------------------------------------------------------------------------

/// Set native result to reg0.
pub unsafe fn set_native_result(result: *mut BoyiaValue, vm: &mut BoyiaVM) {
    if result.is_null() {
        return;
    }
    value_copy(&mut vm.mCpu.mReg0, result);
}

/// Get native result (reg0).
pub unsafe fn get_native_result(vm: &mut BoyiaVM) -> *mut BoyiaValue {
    &mut vm.mCpu.mReg0
}

/// Get native helper result (reg1).
pub unsafe fn get_native_helper_result(vm: &mut BoyiaVM) -> *mut BoyiaValue {
    &mut vm.mCpu.mReg1
}

/// Get current scope local stack size.
pub unsafe fn get_local_size(vm: &mut BoyiaVM) -> LInt {
    if (*vm).mEState.is_null() {
        return 0;
    }
    let e_state = (*vm).mEState;
    if (*e_state).mFrameIndex > 0 {
        let frame_idx = ((*e_state).mFrameIndex - 1) as usize;
        let prev_frame = &(*e_state).mExecStack[frame_idx];
        return (*e_state).mStackFrame.mLValSize - prev_frame.mLValSize;
    }
    (*e_state).mStackFrame.mLValSize
}

/// Get local value by index.
pub unsafe fn get_local_value(idx: LInt, vm: &mut BoyiaVM) -> *mut LVoid {
    let size = get_local_size(vm);
    if idx < 0 || idx >= size {
        return ptr::null_mut();
    }
    if (*vm).mEState.is_null() {
        return ptr::null_mut();
    }
    let e_state = (*vm).mEState;
    let start = if (*e_state).mFrameIndex > 0 {
        let frame_idx = ((*e_state).mFrameIndex - 1) as usize;
        (*e_state).mExecStack[frame_idx].mLValSize
    } else {
        0
    };
    &mut (*e_state).mLocals[start as usize + idx as usize] as *mut BoyiaValue as *mut LVoid
}

/// Reads **local index 0** in the current frame (the callee slot: same layout as `native_call_impl`'s `args[0]`).
///
/// If that value is a function type, returns its [`BoyiaFunction`] pointer, the address of the first
/// closure capture in [`BoyiaFunction::mParams`] (i.e. `mParams + mParamSize`), and [`BoyiaFunction::mCaptureCount`].
/// If there are no captures, `captures` is null and `capture_count` is 0.
pub unsafe fn get_callee_and_captures_from_locals(vm: &mut BoyiaVM) -> CalleeCapturesInfo {
    let mut out = CalleeCapturesInfo {
        callee: ptr::null_mut(),
        captures: ptr::null_mut(),
        capture_count: 0,
    };
    let val = get_local_value(0, vm) as *mut BoyiaValue;
    if val.is_null() {
        return out;
    }
    let vt = (*val).mValueType;
    let is_fn = matches!(
        vt,
        ValueType::BY_FUNC
            | ValueType::BY_PROP_FUNC
            | ValueType::BY_ASYNC_PROP
            | ValueType::BY_NAV_FUNC
            | ValueType::BY_NAV_PROP
            | ValueType::BY_ANONYM_FUNC
    );
    if !is_fn {
        return out;
    }
    let fun = (*val).mValue.mObj.mPtr as *mut BoyiaFunction;
    if fun.is_null() {
        return out;
    }
    out.callee = fun;
    // Only anonymous functions can capture outer variables.
    let n = if vt == ValueType::BY_ANONYM_FUNC {
        (*fun).mCaptureCount
    } else {
        0
    };
    out.capture_count = n;
    if n > 0 && !(*fun).mParams.is_null() {
        let off = (*fun).mParamSize as usize;
        out.captures = (*fun).mParams.add(off);
    }
    out
}

/// Push local value.
pub unsafe fn local_push(value: *mut BoyiaValue, vm: &mut BoyiaVM) {
    if value.is_null() {
        return;
    }
    if (*vm).mEState.is_null() {
        return;
    }
    let e_state = (*vm).mEState;
    if (*e_state).mStackFrame.mLValSize >= NUM_LOCAL_VARS as i32 {
        return;
    }
    let idx = (*e_state).mStackFrame.mLValSize;
    value_copy(&mut (*e_state).mLocals[idx as usize], value);
    (*e_state).mStackFrame.mLValSize += 1;
}

/// Get local stack and optional op stack for one exec-state frame.
/// `state == null` starts from `vm.mESLink`; otherwise `state` points at the current [ExecState].
pub unsafe fn get_local_stack(
    stack: *mut LIntPtr,
    size: *mut LInt,
    op_stack: *mut LIntPtr,
    op_size: *mut LInt,
    vm: &mut BoyiaVM,
    state: *mut LVoid,
) -> *mut LVoid {
    let state = if state.is_null() {
        vm.mESLink
    } else {
        state as *mut ExecState
    };
    if state.is_null() {
        if !size.is_null() {
            *size = 0;
        }
        return ptr::null_mut();
    }
    if !stack.is_null() {
        *stack = (*state).mLocals.as_ptr() as LIntPtr;
    }
    if !size.is_null() {
        *size = (*state).mStackFrame.mLValSize;
    }
    if !op_stack.is_null() {
        *op_stack = (*state).mOpStack.as_ptr() as LIntPtr;
    }
    if !op_size.is_null() {
        *op_size = (*state).mStackFrame.mResultNum;
    }
    (*state).mPrev as *mut LVoid
}

/// Get global table pointer and size.
pub unsafe fn get_global_table(table: *mut LIntPtr, size: *mut LInt, vm: &mut BoyiaVM) {
    if !table.is_null() {
        *table = (*vm).mGlobals as LIntPtr;
    }
    if !size.is_null() {
        *size = (*vm).mGValSize;
    }
}

/// Get creator (runtime) from VM as `*mut dyn Runtime`.
pub unsafe fn get_vm_creator(vm: &mut BoyiaVM) -> *mut dyn Runtime {
    (*vm).mCreator
}

/// Set integer result in reg0.
pub unsafe fn set_int_result(result: LInt, vm: &mut BoyiaVM) -> LInt {
    let mut val = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_INT,
        mValue: RealValue { mIntVal: result as LIntPtr },
    };
    set_native_result(&mut val, vm);
    OpHandleResult::kOpResultSuccess as i32
}

// ---------------------------------------------------------------------------
// Runtime allocation (new_data + zero)
// ---------------------------------------------------------------------------

/// Allocate from runtime with [Runtime::new_data] and zero the block. Returns null if creator is null or allocation fails.
pub(crate) unsafe fn runtime_new_data_zeroed(creator: *mut dyn Runtime, size: LInt) -> *mut LVoid {
    if creator.is_null() || size <= 0 {
        return ptr::null_mut();
    }
    let p = (*creator).new_data(size);
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::write_bytes(p as *mut u8, 0, size as usize);
    p
}

#[inline]
unsafe fn create_exec_state_cache() -> *mut LVoid {
    create_memory_cache(mem::size_of::<ExecState>() as LInt, EXEC_STATE_CAPACITY as LInt)
}

/// Match `CREATE_MEMCACHE(MicroTask, MICRO_TASK_CAPACITY)` in BoyiaCore.cpp `CreateTaskQueue`.
#[inline]
unsafe fn create_micro_task_cache() -> *mut LVoid {
    create_memory_cache(mem::size_of::<MicroTask>() as LInt, MICRO_TASK_CAPACITY as LInt)
}

#[inline]
unsafe fn alloc_exec_state(vm: &mut BoyiaVM) -> *mut ExecState {
    if vm.mEStateCache.is_null() {
        return ptr::null_mut();
    }
    alloc_memory_chunk(vm.mEStateCache) as *mut ExecState
}

#[inline]
unsafe fn free_exec_state(state: *mut ExecState, vm: &mut BoyiaVM) {
    if state.is_null() || vm.mEStateCache.is_null() {
        return;
    }
    free_memory_chunk(state as *mut LVoid, vm.mEStateCache);
}

/// Extra slots when an Array-style `mParams` buffer is full. Match `addElementToVector` in BoyiaLib.cpp.
const BOYIA_VECTOR_GROW_DELTA: LInt = 10;

/// Grow `fun->mParams` when `mParamSize >= GET_FUNCTION_COUNT(fun)`. Matches `addElementToVector` in BoyiaLib.cpp:
/// allocate `count + 10` slots, copy `count * sizeof(BoyiaValue)` from the old buffer, `delete_data` old buffer,
/// update `mParamCount` lower 16 bits to the new capacity (preserving high 16 bits).
///
/// Returns `false` if the VM/creator is invalid or allocation fails.
pub unsafe fn vector_params_grow_if_full(fun: *mut BoyiaFunction, vm: &mut BoyiaVM) -> bool {
    if fun.is_null() {
        return false;
    }
    let cap = get_function_count(fun);
    if (*fun).mParamSize < cap {
        return true;
    }
    let creator = vm.mCreator;
    if creator.is_null() {
        return false;
    }
    let old_ptr = (*fun).mParams;
    let count = cap;
    if count > 0 && old_ptr.is_null() {
        return false;
    }
    let new_cap = match count.checked_add(BOYIA_VECTOR_GROW_DELTA) {
        Some(n) => n,
        None => return false,
    };
    let new_bytes = match (new_cap as usize).checked_mul(mem::size_of::<BoyiaValue>()) {
        Some(n) if n <= LInt::MAX as usize => n,
        _ => return false,
    };
    let new_ptr = runtime_new_data_zeroed(creator, new_bytes as LInt) as *mut BoyiaValue;
    if new_ptr.is_null() {
        return false;
    }
    let copy_bytes = (count as usize).saturating_mul(mem::size_of::<BoyiaValue>());
    if copy_bytes > 0 {
        ptr::copy_nonoverlapping(old_ptr as *const u8, new_ptr as *mut u8, copy_bytes);
    }
    if !old_ptr.is_null() {
        (*creator).delete_data(old_ptr as *mut LVoid);
    }
    (*fun).mParams = new_ptr;
    let high = (*fun).mParamCount & !0x0000_FFFF;
    (*fun).mParamCount = high | (new_cap & 0x0000_FFFF);
    true
}

/// Match `BoyiaCore.cpp` `CloneAnonymBoyiaFunctionForPushArg`: heap `BoyiaFunction` + `mParams` (`NUM_FUNC_PARAMS`),
/// copy `mParamCount` / `mFuncBody` / `mParamSize`, `mCaptureCount = 0`, `ValueCopy` formal slots `[0..mParamSize)`.
pub(crate) unsafe fn clone_anonym_boyia_function_for_push_arg(
    src: *mut BoyiaFunction,
    vm: &mut BoyiaVM,
) -> *mut BoyiaFunction {
    if src.is_null() {
        return ptr::null_mut();
    }
    let creator = vm.mCreator;
    if creator.is_null() {
        return ptr::null_mut();
    }
    let dst = runtime_new_data_zeroed(creator, mem::size_of::<BoyiaFunction>() as LInt) as *mut BoyiaFunction;
    if dst.is_null() {
        return ptr::null_mut();
    }
    let params_bytes = (NUM_FUNC_PARAMS * mem::size_of::<BoyiaValue>()) as LInt;
    (*dst).mParams = runtime_new_data_zeroed(creator, params_bytes) as *mut BoyiaValue;
    if (*dst).mParams.is_null() {
        (*creator).delete_data(dst as *mut LVoid);
        return ptr::null_mut();
    }
    (*dst).mParamCount = (*src).mParamCount;
    (*dst).mFuncBody = (*src).mFuncBody;
    (*dst).mParamSize = (*src).mParamSize;
    (*dst).mCaptureCount = 0;
    if !(*src).mParams.is_null() && (*src).mParamSize > 0 {
        for i in 0..(*src).mParamSize as usize {
            value_copy((*dst).mParams.add(i), (*src).mParams.add(i));
        }
    }
    dst
}

// ---------------------------------------------------------------------------
// Init / Destroy VM
// ---------------------------------------------------------------------------

/// Last fully successful [`init_vm`] milestone. Variants are ordered for rollback: later stages imply earlier allocations exist.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InitVmStage {
    VmShell,
    GlobalsOk,
    FunTableOk,
    CpuOk,
    ExecStateCacheOk,
    /// `create_exec_state` + `switch_exec_state` done.
    ExecStateActive,
    /// `MicroTaskQueue` struct allocated (`mTaskCache` may be null if cache create failed).
    TaskQueueShell,
}

/// `completed` = last fully successful milestone. Frees in reverse order; always returns null.
unsafe fn init_vm_abort(vm_ptr: *mut BoyiaVM, completed: InitVmStage) -> *mut LVoid {
    if vm_ptr.is_null() {
        return ptr::null_mut();
    }
    let vm = &mut *vm_ptr;
    let globals_layout = Layout::array::<BoyiaValue>(NUM_GLOBAL_VARS).unwrap();
    let fun_table_layout = Layout::array::<BoyiaFunction>(NUM_FUNC).unwrap();
    let vm_layout = Layout::new::<BoyiaVM>();
    let task_queue_layout = Layout::new::<MicroTaskQueue>();

    if completed >= InitVmStage::TaskQueueShell && !vm.mTaskQueue.is_null() {
        let q = vm.mTaskQueue;
        if !(*q).mTaskCache.is_null() {
            destroy_memory_cache((*q).mTaskCache);
            (*q).mTaskCache = ptr::null_mut();
        }
        dealloc(q as *mut u8, task_queue_layout);
        vm.mTaskQueue = ptr::null_mut();
    }
    if completed >= InitVmStage::ExecStateCacheOk && !vm.mEStateCache.is_null() {
        destroy_memory_cache(vm.mEStateCache);
        vm.mEStateCache = ptr::null_mut();
    }
    if completed >= InitVmStage::FunTableOk && !vm.mFunTable.is_null() {
        dealloc(vm.mFunTable as *mut u8, fun_table_layout);
        vm.mFunTable = ptr::null_mut();
    }
    if completed >= InitVmStage::GlobalsOk && !vm.mGlobals.is_null() {
        dealloc(vm.mGlobals as *mut u8, globals_layout);
        vm.mGlobals = ptr::null_mut();
    }
    if completed >= InitVmStage::VmShell {
        ptr::drop_in_place(vm.vm_code_mut());
        ptr::drop_in_place(&mut vm.mStrTable);
        ptr::drop_in_place(&mut vm.mEntry);
        dealloc(vm_ptr as *mut u8, vm_layout);
    }
    ptr::null_mut()
}

/// Initialize VM with runtime (creator). Returns null on failure.
pub unsafe fn init_vm(creator: *mut dyn Runtime) -> *mut LVoid {
    init_vm_boxed(creator)
        .map(|vm| Box::into_raw(vm) as *mut LVoid)
        .unwrap_or(ptr::null_mut())
}

/// Initialize VM with runtime (creator). Returns `None` on failure.
pub unsafe fn init_vm_boxed(creator: *mut dyn Runtime) -> Option<Box<BoyiaVM>> {
    eprintln!("[init_vm] 1 alloc BoyiaVM");
    let layout = Layout::new::<BoyiaVM>();
    let vm_ptr = alloc_zeroed(layout) as *mut BoyiaVM;
    if vm_ptr.is_null() {
        eprintln!("[init_vm] ERROR vm_ptr null");
        return None;
    }
    let vm = &mut *vm_ptr;
    vm.mCreator = creator;
    vm.mESLink = ptr::null_mut();
    vm.mGValSize = 0;
    vm.mFunSize = 0;
    *vm.vm_code_mut() = VMCode::new();
    vm.mStrTable = VMStrTable::new();
    vm.mEntry = VMEntryTable::new();

    eprintln!("[init_vm] 2 alloc Globals");
    let globals_layout = Layout::array::<BoyiaValue>(NUM_GLOBAL_VARS).unwrap();
    vm.mGlobals = alloc_zeroed(globals_layout) as *mut BoyiaValue;
    if vm.mGlobals.is_null() {
        eprintln!("[init_vm] ERROR mGlobals alloc null");
        init_vm_abort(vm_ptr, InitVmStage::VmShell);
        return None;
    }

    eprintln!("[init_vm] 3 alloc FunTable");
    let fun_table_layout = Layout::array::<BoyiaFunction>(NUM_FUNC).unwrap();
    vm.mFunTable = alloc_zeroed(fun_table_layout) as *mut BoyiaFunction;
    if vm.mFunTable.is_null() {
        init_vm_abort(vm_ptr, InitVmStage::GlobalsOk);
        return None;
    }

    eprintln!("[init_vm] 4 init Cpu");
    vm.mCpu.mReg0 = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_ARG,
        mValue: RealValue { mIntVal: 0 },
    };
    vm.mCpu.mReg1 = BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_ARG,
        mValue: RealValue { mIntVal: 0 },
    };
    eprintln!("[init_vm] 4b cpu inited");

    eprintln!("[init_vm] 5 init embedded tables (VMCode/StrTable/Entry)");

    eprintln!("[init_vm] 6 alloc ExecState");
    vm.mEStateCache = create_exec_state_cache();
    if vm.mEStateCache.is_null() {
        init_vm_abort(vm_ptr, InitVmStage::CpuOk);
        return None;
    }
    vm.mEState = create_exec_state(vm);
    if vm.mEState.is_null() {
        init_vm_abort(vm_ptr, InitVmStage::ExecStateCacheOk);
        return None;
    }
    switch_exec_state(vm.mEState, vm);

    eprintln!("[init_vm] 9 alloc TaskQueue");
    let task_queue_layout = Layout::new::<MicroTaskQueue>();
    vm.mTaskQueue = alloc_zeroed(task_queue_layout) as *mut MicroTaskQueue;
    if vm.mTaskQueue.is_null() {
        init_vm_abort(vm_ptr, InitVmStage::ExecStateActive);
        return None;
    }
    (*vm.mTaskQueue).mUsedTasks.mHead = ptr::null_mut();
    (*vm.mTaskQueue).mUsedTasks.mEnd = ptr::null_mut();
    (*vm.mTaskQueue).mAllocTasks.mHead = ptr::null_mut();
    (*vm.mTaskQueue).mAllocTasks.mEnd = ptr::null_mut();
    (*vm.mTaskQueue).mTaskCache = create_micro_task_cache();
    if (*vm.mTaskQueue).mTaskCache.is_null() {
        init_vm_abort(vm_ptr, InitVmStage::TaskQueueShell);
        return None;
    }
    eprintln!("[init_vm] 10 done");
    Some(Box::from_raw(vm_ptr))
}

unsafe fn drop_vm_owned_resources(vm: &mut BoyiaVM) {
    if !vm.mGlobals.is_null() {
        let layout = Layout::array::<BoyiaValue>(NUM_GLOBAL_VARS).unwrap();
        dealloc(vm.mGlobals as *mut u8, layout);
        vm.mGlobals = ptr::null_mut();
    }
    if !vm.mFunTable.is_null() {
        // mParams of each function are from memory pool; only free the table array.
        let layout = Layout::array::<BoyiaFunction>(NUM_FUNC).unwrap();
        dealloc(vm.mFunTable as *mut u8, layout);
        vm.mFunTable = ptr::null_mut();
    }
    if !vm.mEStateCache.is_null() {
        destroy_memory_cache(vm.mEStateCache);
        vm.mEStateCache = ptr::null_mut();
    }
    if !vm.mTaskQueue.is_null() {
        let q = vm.mTaskQueue;
        if !(*q).mTaskCache.is_null() {
            destroy_memory_cache((*q).mTaskCache);
            (*q).mTaskCache = ptr::null_mut();
        }
        let layout = Layout::new::<MicroTaskQueue>();
        dealloc(q as *mut u8, layout);
        vm.mTaskQueue = ptr::null_mut();
    }
    // Embedded tables (`mVMCode`, `mStrTable`, `mEntry`) are dropped by Rust after `Drop::drop` returns.
}

impl Drop for BoyiaVM {
    fn drop(&mut self) {
        unsafe {
            drop_vm_owned_resources(self);
        }
    }
}

/// Destroy VM and free all allocated memory.
pub unsafe fn destroy_vm(vm: *mut LVoid) {
    if vm.is_null() {
        return;
    }
    drop(Box::from_raw(vm as *mut BoyiaVM));
}

// ---------------------------------------------------------------------------
// Load string table / instructions / entry table
// ---------------------------------------------------------------------------

pub unsafe fn load_string_table(string_table: *mut BoyiaStr, size: LInt, vm: &mut BoyiaVM) {
    if string_table.is_null() || size <= 0 {
        return;
    }
    let str_table = &mut (*vm).mStrTable;
    let copy_size = if size as usize > CONST_CAPACITY {
        CONST_CAPACITY
    } else {
        size as usize
    };
    str_table.load_from_buffer(string_table, copy_size);
}

pub unsafe fn load_instructions(buffer: *mut LVoid, size: LInt, vm: &mut BoyiaVM) {
    if buffer.is_null() || size <= 0 {
        return;
    }
    let vmcode = vm.vm_code_mut();
    let instruction_size = mem::size_of::<Instruction>();
    let count = (size as usize) / instruction_size;
    vmcode.load_from_buffer(buffer as *const Instruction, count);
}

pub unsafe fn load_entry_table(buffer: *mut LVoid, size: LInt, vm: &mut BoyiaVM) {
    if buffer.is_null() || size <= 0 {
        return;
    }
    let entry = &mut (*vm).mEntry;
    let int_size = mem::size_of::<LInt>();
    let count = (size as usize) / int_size;
    entry.load_from_buffer(buffer as *const LInt, count);
}

// ---------------------------------------------------------------------------
// Helpers: find_global, init_function, create_fun_val, copy_function
// ---------------------------------------------------------------------------

pub(crate) unsafe fn find_global(key: LUintPtr, vm: &mut BoyiaVM) -> *mut BoyiaValue {
    println!("[find_global] key={}", key);
    if vm.mGlobals.is_null() {
        return ptr::null_mut();
    }
    for i in 0..vm.mGValSize as usize {
        if vm.mGlobals.add(i).read().mNameKey == key {
            return vm.mGlobals.add(i);
        }
    }

    println!("[find_global] key={} get nullptr", key);
    ptr::null_mut()
}

fn is_object_prop_func(type_: ValueType) -> bool {
    type_ == ValueType::BY_PROP_FUNC || type_ == ValueType::BY_ASYNC_PROP || type_ == ValueType::BY_NAV_PROP || type_ == ValueType::BY_ANONYM_FUNC
}

pub(crate) unsafe fn init_function(fun: *mut BoyiaFunction, vm: &mut BoyiaVM) {
    eprintln!("[init_function] fun={:?}", fun);
    if fun.is_null() {
        eprintln!("[init_function] null arg");
        return;
    }
    (*fun).mParamSize = 0;
    (*fun).mCaptureCount = 0;
    let creator = vm.mCreator;
    if creator.is_null() {
        eprintln!("[init_function] ERROR mCreator null");
        return;
    }
    let params_size = (NUM_FUNC_PARAMS as usize) * mem::size_of::<BoyiaValue>();
    (*fun).mParams = runtime_new_data_zeroed(creator, params_size as LInt) as *mut BoyiaValue;
    eprintln!("[init_function] mParams={:?}", (*fun).mParams);
    if (*fun).mParams.is_null() {
        eprintln!("[init_function] ERROR mParams alloc null");
        return;
    }
    (*fun).mParamCount = NUM_FUNC_PARAMS as LInt;
    vm.mFunSize += 1;
    eprintln!("[init_function] done mFunSize={}", vm.mFunSize);
}

pub(crate) unsafe fn create_fun_val(hash_key: LUintPtr, type_: ValueType, vm: &mut BoyiaVM) -> *mut BoyiaValue {
    eprintln!("[create_fun_val] key={} type={} mGValSize={} mFunSize={}", hash_key, type_ as u8, vm.mGValSize, vm.mFunSize);
    if vm.mGlobals.is_null() || vm.mFunTable.is_null() {
        eprintln!("[create_fun_val] null vm/globals/fun_table");
        return ptr::null_mut();
    }
    if vm.mGValSize as usize >= NUM_GLOBAL_VARS || vm.mFunSize as usize >= NUM_FUNC {
        eprintln!("[create_fun_val] capacity full");
        return ptr::null_mut();
    }
    let val = vm.mGlobals.add(vm.mGValSize as usize);
    vm.mGValSize += 1;
    let fun = vm.mFunTable.add(vm.mFunSize as usize);
    eprintln!("[create_fun_val] val={:?} fun={:?} type={:?} hash_key={:?}", val, fun, type_ as u8, hash_key);
    (*val).mValueType = type_;
    (*val).mNameKey = hash_key;
    (*val).mValue.mObj.mPtr = fun as LIntPtr;
    (*val).mValue.mObj.mSuper = K_BOYIA_NULL;
    if type_ == ValueType::BY_CLASS {
        (*fun).mFuncBody = val as LIntPtr;
    }
    init_function(fun, vm);
    eprintln!("[create_fun_val] done");
    val
}

unsafe fn copy_function(
    cls_val: *const BoyiaValue,
    count: LInt,
    vm: &mut BoyiaVM,
) -> *mut BoyiaFunction {
    if cls_val.is_null() || count <= 0 {
        return ptr::null_mut();
    }
    let func = (*cls_val).mValue.mObj.mPtr as *mut BoyiaFunction;
    if func.is_null() {
        return ptr::null_mut();
    }
    let creator = vm.mCreator;
    if creator.is_null() {
        return ptr::null_mut();
    }
    let new_func = runtime_new_data_zeroed(creator, mem::size_of::<BoyiaFunction>() as LInt) as *mut BoyiaFunction;
    if new_func.is_null() {
        return ptr::null_mut();
    }
    let params_size = (count as usize) * mem::size_of::<BoyiaValue>();
    (*new_func).mParams = runtime_new_data_zeroed(creator, params_size as LInt) as *mut BoyiaValue;
    if (*new_func).mParams.is_null() {
        (*creator).delete_data(new_func as *mut LVoid);
        return ptr::null_mut();
    }
    (*new_func).mParamSize = 0;
    (*new_func).mFuncBody = (*func).mFuncBody;
    (*new_func).mParamCount = count;
    let mut cls_val = cls_val as *const BoyiaValue;
    while !cls_val.is_null() {
        let func = (*cls_val).mValue.mObj.mPtr as *mut BoyiaFunction;
        if func.is_null() {
            break;
        }
        let mut idx = (*func).mParamSize;
        while idx > 0 {
            idx -= 1;
            let type_ = (*func).mParams.add(idx as usize).read().mValueType;
            if type_ == ValueType::BY_FUNC || type_ == ValueType::BY_NAV_FUNC {
                continue;
            }
            let prop = (*new_func).mParams.add((*new_func).mParamSize as usize);
            value_copy(prop, (*func).mParams.add(idx as usize));
            if is_object_prop_func(type_) {
                (*prop).mValue.mObj.mSuper = new_func as LIntPtr;
            }
            (*new_func).mParamSize += 1;
        }
        cls_val = (*cls_val).mValue.mObj.mSuper as *const BoyiaValue;
    }
    new_func
}

// ---------------------------------------------------------------------------
// CompileCode, CreateObject, CacheVMCode, ExecuteGlobalCode, CopyObject, NativeCallImpl, CreateGlobalClass
// ---------------------------------------------------------------------------

/// Compile Boyia source: parse top-level var/fun/class and register in VM.
/// Does not emit bytecode; use LoadInstructions/LoadEntryTable for pre-compiled code.
pub unsafe fn compile_code(code: *mut LInt8, vm: &mut BoyiaVM) {
    if code.is_null() {
        return;
    }
    crate::compile::parse_and_register(code, vm);
}

/// Create object from local 0 class; set reg0.
pub unsafe fn create_object(vm: &mut BoyiaVM) -> LInt {
    eprintln!("[create_object] called");
    let value = get_local_value(0, vm) as *mut BoyiaValue;
    if value.is_null() {
        eprintln!("[create_object] -> kOpResultEnd (local 0 null)");
        return OpHandleResult::kOpResultEnd as i32;
    }
    let value_type = (*value).mValueType;
    let class_name_key = (*value).mNameKey;
    eprintln!(
        "[create_object] class: local0 ptr={:?}, mValueType={} (BY_CLASS=13), mNameKey={}",
        value, value_type as u8, class_name_key
    );
    if value_type != ValueType::BY_CLASS {
        eprintln!("[create_object] -> kOpResultEnd (local 0 not BY_CLASS)");
        return OpHandleResult::kOpResultEnd as i32;
    }
    value_copy(&mut vm.mCpu.mReg0, value);
    let new_func = copy_function(value, NUM_FUNC_PARAMS as LInt, vm);
    if new_func.is_null() {
        eprintln!("[create_object] -> kOpResultEnd (copy_function returned null)");
        return OpHandleResult::kOpResultEnd as i32;
    }
    vm.mCpu.mReg0.mValue.mObj.mPtr = new_func as LIntPtr;
    vm.mCpu.mReg0.mValue.mObj.mSuper = (*value).mValue.mObj.mSuper;
    eprintln!(
        "[create_object] -> kOpResultSuccess (instance ptr={:?}, class key={})",
        new_func, class_name_key
    );

    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return OpHandleResult::kOpResultEnd as i32;
    }
    (*rt).gc_append_ref(new_func as *mut LVoid, ValueType::BY_CLASS);
    OpHandleResult::kOpResultSuccess as i32
}

/// Cache VM code: clear inline caches, patch instructions.
pub unsafe fn cache_vm_code(vm: &mut BoyiaVM) {
    let vmcode = vm.vm_code_mut();
    if vmcode.is_empty() {
        return;
    }
    let size = vmcode.len();
    for i in 0..size {
        let Some(inst) = vmcode.instruction_mut(i) else {
            continue;
        };
        if !inst.mCache.is_null() {
            let layout = Layout::new::<InlineCache>();
            dealloc(inst.mCache as *mut u8, layout);
            inst.mCache = ptr::null_mut();
        }
        if inst.mOPCode == CmdType::kCmdCreateFunction
            && inst.mOPRight.int_value() as u8 == ValueType::BY_ANONYM_FUNC as u8
        {
            inst.mOPLeft.mType = OpType::OP_NONE;
            inst.mOPLeft.set_int_value(0);
        }
        if inst.mOPCode == CmdType::kCmdOnceJmpTrue {
            inst.mOPLeft.mType = OpType::OP_CONST_NUMBER;
            inst.mOPLeft.set_int_value(LTrue as LIntPtr);
        }
    }
}

/// Copy object by global key and size.
pub unsafe fn copy_object(hash_key: LUintPtr, size: LInt, vm: &mut BoyiaVM) -> *mut LVoid {
    if size <= 0 {
        return ptr::null_mut();
    }
    let global = find_global(hash_key, vm);
    if global.is_null() {
        return ptr::null_mut();
    }
    let obj_body = copy_function(global, size, vm);
    if obj_body.is_null() {
        return ptr::null_mut();
    }

    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return ptr::null_mut();
    }

    (*rt).gc_append_ref(obj_body as *mut LVoid, ValueType::BY_CLASS);
    obj_body as *mut LVoid
}

/// Get class name key from a BY_CLASS object. Match GetBoyiaClassId in BoyiaValue.cpp.
pub unsafe fn get_boyia_class_id(obj: *const BoyiaValue) -> LUintPtr {
    if obj.is_null() || (*obj).mValueType != ValueType::BY_CLASS {
        return 0;
    }
    let obj_body = (*obj).mValue.mObj.mPtr as *const BoyiaFunction;
    if obj_body.is_null() {
        return 0;
    }
    let clzz = (*obj_body).mFuncBody as *const BoyiaValue;
    if clzz.is_null() {
        return 0;
    }
    (*clzz).mNameKey
}

/// Create and link a new ExecState for callback invocation. Match CreateExecState in BoyiaCore.cpp.
pub(crate) unsafe fn create_exec_state(vm: &mut BoyiaVM) -> *mut ExecState {
    let state = alloc_exec_state(vm);
    if state.is_null() {
        return ptr::null_mut();
    }
    crate::execute::reset_scene(state);
    (*state).mNext = ptr::null_mut();
    (*state).mPrev = vm.mESLink;
    if !vm.mESLink.is_null() {
        (*vm.mESLink).mNext = state;
    }
    vm.mESLink = state;
    state
}

/// Native call impl. Strict port of NativeCallImpl in BoyiaCore.cpp.
pub unsafe fn native_call_impl(
    args: *mut BoyiaValue,
    argc: LInt,
    obj: *mut BoyiaValue,
    vm: &mut BoyiaVM,
) -> OpHandleResult {
    if args.is_null() {
        return OpHandleResult::kOpResultSuccess;
    }
    let current = (*vm).mEState;
    // 第一个参数为调用该函数的函数指针
    let value = args.add(0);
    eprintln!("[native_call_impl] HandlePushParams functionName={}", (*value).mValueType as u8);
    let state = create_exec_state(vm);
    if state.is_null() {
        return OpHandleResult::kOpResultEnd;
    }
    switch_exec_state(state, vm);
    let _ = crate::execute::handle_push_scene(None, vm);
    let start = (*state).mExecStack.as_ptr().add((*state).mFrameIndex as usize - 1).read().mLValSize;
    for idx in 0..argc {
        value_copy(
            (*state).mLocals.as_mut_ptr().add((start + idx) as usize),
            args.add(idx as usize),
        );
    }
    (*state).mStackFrame.mLValSize = argc;
    let func = (*value).mValue.mObj.mPtr as *mut BoyiaFunction;
    if (*value).mValueType == ValueType::BY_NAV_PROP {
        local_push(obj, vm);
        let nav_fun = std::mem::transmute::<_, NativePtr>((*func).mFuncBody);
        nav_fun(vm);
    } else {
        crate::execute::assign_state_class((*vm).mEState, obj);
        let cmds = (*func).mFuncBody as *const CommandTable;
        (*(*vm).mEState).mStackFrame.mContext = cmds as *mut CommandTable;
        (*(*vm).mEState).mStackFrame.mPC = (*cmds).mBegin;
        crate::execute::exec_instruction(vm);
    }
    destroy_exec_state(state, vm);
    switch_exec_state(current, vm);
    OpHandleResult::kOpResultSuccess
}

/// Get runtime as `*mut dyn Runtime` from VM ([BoyiaVM::mCreator]).
pub unsafe fn get_runtime_from_vm(vm: &mut BoyiaVM) -> *mut dyn Runtime {
    get_vm_creator(vm)
}

/// Find native function index by name key. Uses [get_runtime_from_vm] to get [Runtime] and calls [Runtime::find_native_func]. Crate-internal only.
pub(crate) unsafe fn find_native_func(vm: &mut BoyiaVM, key: LUintPtr) -> LInt {
    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return -1;
    }
    (*rt).find_native_func(key)
}

/// Call native function by index. Uses [get_runtime_from_vm] to get [Runtime] and calls [Runtime::call_native_function].
pub unsafe fn native_call_by_index(vm: &mut BoyiaVM, idx: LInt) -> LInt {
    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return OpHandleResult::kOpResultSuccess as i32;
    }
    (&mut *rt).call_native_function(vm, idx)
}

/// Get identifier for a string buffer via [Runtime::gen_ident_by_str].
pub unsafe fn gen_identifier_from_str(vm: &mut BoyiaVM, s: *const BoyiaStr) -> LUintPtr {
    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return 0;
    }
    (*rt).gen_ident_by_str(s)
}

/// Reverse identifier key → string (C++ `GetIdentName`). Used by Json `toString` / Map serialization.
pub unsafe fn name_for_identifier(vm: &mut BoyiaVM, id: LUintPtr) -> Option<String> {
    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return None;
    }
    (*rt).name_for_identifier(id)
}

/// Create global class value.
pub unsafe fn create_global_class(key: LUintPtr, vm: &mut BoyiaVM) -> *mut LVoid {
    eprintln!("[create_global_class] key={}", key);
    let val = create_fun_val(key, ValueType::BY_CLASS, vm) as *mut LVoid;
    eprintln!("[create_global_class] done val={:?}", val);
    val
}

/// MicroTask class name key (for HandleAwait). Per BoyiaValue.h BuiltinId::kBoyiaMicroTask = 6.
pub(crate) unsafe fn micro_task_class_key(_vm: &mut BoyiaVM) -> LUintPtr {
    BuiltinId::kBoyiaMicroTask.as_key()
}

/// Create a MicroTask instance object. Match CreateMicroTaskObject in BoyiaValue.cpp.
pub(crate) unsafe fn create_micro_task_object(vm: &mut BoyiaVM) -> *mut BoyiaFunction {
    copy_object(micro_task_class_key(vm), 32, vm) as *mut BoyiaFunction
}

// ---------------------------------------------------------------------------
// Builtin helpers (used by builtins module; match BoyiaValue.cpp)
// ---------------------------------------------------------------------------

/// Allocate a function slot for a builtin method (mFuncBody = native_ptr). Returns null if no slot.
pub unsafe fn alloc_builtin_function(vm: &mut BoyiaVM, native_ptr: NativePtr) -> *mut BoyiaFunction {
    if (*vm).mFunSize as usize >= NUM_FUNC {
        eprintln!("[alloc_builtin_function] mFunSize full {}", (*vm).mFunSize);
        return ptr::null_mut();
    }
    let fun = vm.mFunTable.add(vm.mFunSize as usize);
    vm.mFunSize += 1;
    // slot already zeroed from init_vm alloc_zeroed(mFunTable)
    (*fun).mFuncBody = native_ptr as LIntPtr;
    (*fun).mParams = ptr::null_mut();
    (*fun).mParamSize = 0;
    (*fun).mParamCount = 0;
    fun
}

/// String buffer: [`ValueType::BY_STRING`] (inline [`BoyiaStr`]), or [`ValueType::BY_CLASS`] with
/// [`BuiltinId::kBoyiaString`] (object `mParams[1].mStrVal`). Otherwise null — do not treat arbitrary
/// `BY_CLASS` as [`BoyiaFunction`] string bodies. Match `GetStringBuffer` for real string values only.
pub unsafe fn get_string_buffer(ref_: *const BoyiaValue) -> *const BoyiaStr {
    if ref_.is_null() {
        return ptr::null();
    }
    match (*ref_).mValueType {
        ValueType::BY_STRING => ptr::addr_of!((*ref_).mValue.mStrVal),
        ValueType::BY_CLASS if get_boyia_class_id(ref_) == BuiltinId::kBoyiaString.as_key() => {
            let obj = (*ref_).mValue.mObj.mPtr as *mut BoyiaFunction;
            if obj.is_null() {
                return ptr::null();
            }
            get_string_buffer_from_body(obj)
        }
        _ => ptr::null(),
    }
}

/// String buffer from function body (body->mParams[1].mValue.mStrVal). Match GetStringBufferFromBody in BoyiaValue.cpp.
pub unsafe fn get_string_buffer_from_body(body: *mut BoyiaFunction) -> *mut BoyiaStr {
    if body.is_null() {
        return ptr::null_mut();
    }
    let p = (*body).mParams;
    if p.is_null() {
        return ptr::null_mut();
    }
    ptr::addr_of_mut!((*p.add(1)).mValue.mStrVal)
}

/// String hash from a BY_CLASS string value (object's mParams[0].mIntVal). Match GetStringHash.
pub unsafe fn get_string_hash(ref_: *const BoyiaValue) -> LIntPtr {
    if ref_.is_null() {
        return 0;
    }
    let obj = (*ref_).mValue.mObj.mPtr as *const BoyiaFunction;
    if obj.is_null() {
        return 0;
    }
    (*obj).mParams.add(0).read().mValue.mIntVal
}

#[inline]
pub(crate) unsafe fn is_boyia_number(v: *const BoyiaValue) -> bool {
    if v.is_null() {
        return false;
    }
    matches!(
        (*v).mValueType,
        ValueType::BY_INT | ValueType::BY_CHAR | ValueType::BY_REAL
    )
}

#[inline]
pub(crate) unsafe fn get_boyia_number(v: *const BoyiaValue) -> LReal64 {
    match (*v).mValueType {
        ValueType::BY_INT | ValueType::BY_CHAR => (*v).mValue.mIntVal as LReal64,
        ValueType::BY_REAL => (*v).mValue.mRealVal,
        _ => 0.0,
    }
}

/// Compare two BoyiaValue for equality (match CompareValue in BoyiaValue.cpp).
pub unsafe fn compare_value(src: *const BoyiaValue, dest: *const BoyiaValue) -> bool {
    if src.is_null() || dest.is_null() {
        return false;
    }
    if is_boyia_number(src)
        && is_boyia_number(dest)
        && get_boyia_number(src) == get_boyia_number(dest)
    {
        return true;
    }
    if (*src).mValueType != (*dest).mValueType {
        return false;
    }
    match (*src).mValueType {
        ValueType::BY_CHAR | ValueType::BY_INT | ValueType::BY_NAVCLASS => {
            (*src).mValue.mIntVal == (*dest).mValue.mIntVal
        }
        ValueType::BY_REAL => (*src).mValue.mRealVal == (*dest).mValue.mRealVal,
        ValueType::BY_FUNC => {
            (*src).mValue.mObj.mPtr == (*dest).mValue.mObj.mPtr
        }
        ValueType::BY_CLASS => {
            if (*src).mValue.mObj.mPtr == (*dest).mValue.mObj.mPtr {
                return true;
            }

            let string_key = BuiltinId::kBoyiaString.as_key();
            if get_boyia_class_id(src) != string_key || get_boyia_class_id(dest) != string_key {
                return false;
            }

            println!("call compare_value");
            let a = get_string_buffer(src);
            let b = get_string_buffer(dest);
            if a.is_null() || b.is_null() {
                return a.is_null() && b.is_null();
            }
            if (*a).mLen != (*b).mLen {
                return false;
            }

            if get_string_hash(src) != get_string_hash(dest) {
                return false;
            }
            let len = (*a).mLen.max(0) as usize;
            std::slice::from_raw_parts((*a).mPtr as *const u8, len)
                == std::slice::from_raw_parts((*b).mPtr as *const u8, len)
        }
        _ => false,
    }
}

/// Simple hash for string buffer (match GenHashCode semantics).
pub fn gen_hash_code(buffer: *const LInt8, len: LInt) -> LIntPtr {
    if buffer.is_null() || len <= 0 {
        return 0;
    }
    let mut hash: LIntPtr = 5381;
    for i in 0..len as usize {
        let c = unsafe { *buffer.add(i) } as LIntPtr;
        hash = hash.wrapping_mul(33).wrapping_add(c);
    }
    hash
}

/// StringBufferType (BoyiaValue.h): mark in mParamCount >> 18.
const K_BOYIA_STRING_BUFFER: LInt = 0x0;
const K_NATIVE_STRING_BUFFER: LInt = 0x1;
const K_CONST_STRING_BUFFER: LInt = 0x2;
const K_STRING_BUFFER_SHIFT: LInt = 18;

/// Max length for int-to-string buffer (decimal digits for LInt). Match MAX_INT_LEN in BoyiaValue.cpp.
const MAX_INT_STR_LEN: LInt = 24;

/// Fetch string from a value: BY_INT / BY_REAL -> pool buffer via [runtime_new_data_zeroed] (caller must [Runtime::delete_data] after use); string object -> borrows [GetStringBuffer]. Match FetchString in BoyiaValue.cpp.
pub(crate) unsafe fn fetch_string(str_out: *mut BoyiaStr, value: *const BoyiaValue, vm: &mut BoyiaVM) {
    if str_out.is_null() || value.is_null() {
        return;
    }
    if (*value).mValueType == ValueType::BY_INT {
        let creator = vm.mCreator;
        let buf = runtime_new_data_zeroed(creator, MAX_INT_STR_LEN) as *mut LInt8;
        if buf.is_null() {
            (*str_out).mPtr = ptr::null_mut();
            (*str_out).mLen = 0;
            return;
        }
        let s = format!("{}", (*value).mValue.mIntVal);
        let bytes = s.as_bytes();
        let len = bytes.len().min(MAX_INT_STR_LEN as usize);
        for (i, &b) in bytes.iter().take(len).enumerate() {
            *buf.add(i) = b as LInt8;
        }
        (*str_out).mPtr = buf;
        (*str_out).mLen = len as LInt;
    } else if (*value).mValueType == ValueType::BY_REAL {
        let creator = vm.mCreator;
        let buf = runtime_new_data_zeroed(creator, MAX_INT_STR_LEN) as *mut LInt8;
        if buf.is_null() {
            (*str_out).mPtr = ptr::null_mut();
            (*str_out).mLen = 0;
            return;
        }
        let s = format!("{}", (*value).mValue.mRealVal);
        let bytes = s.as_bytes();
        let len = bytes.len().min(MAX_INT_STR_LEN as usize);
        for (i, &b) in bytes.iter().take(len).enumerate() {
            *buf.add(i) = b as LInt8;
        }
        (*str_out).mPtr = buf;
        (*str_out).mLen = len as LInt;
    } else {
        let buf = get_string_buffer(value);
        if buf.is_null() {
            (*str_out).mPtr = ptr::null_mut();
            (*str_out).mLen = 0;
            return;
        }
        (*str_out).mPtr = (*buf).mPtr;
        (*str_out).mLen = (*buf).mLen;
    }
}

/// Release temp buffer from [`fetch_string`] when `source` was `BY_INT` or `BY_REAL` (pool alloc). String/object views must not be freed here.
unsafe fn release_fetch_string_temp(s: BoyiaStr, source: *const BoyiaValue, vm: &mut BoyiaVM) {
    if source.is_null() {
        return;
    }
    let vt = (*source).mValueType;
    if vt != ValueType::BY_INT && vt != ValueType::BY_REAL {
        return;
    }
    if s.mPtr.is_null() {
        return;
    }
    let creator = vm.mCreator;
    if creator.is_null() {
        return;
    }
    (*creator).delete_data(s.mPtr as *mut LVoid);
}

/// String concatenation: left + right, result stored in right (R0). Match StringAdd in BoyiaValue.cpp.
pub(crate) unsafe fn string_add(left: *const BoyiaValue, right: *mut BoyiaValue, vm: &mut BoyiaVM) {
    if left.is_null() || right.is_null() {
        return;
    }
    let mut left_str = BoyiaStr {
        mPtr: ptr::null_mut(),
        mLen: 0,
    };
    let mut right_str = BoyiaStr {
        mPtr: ptr::null_mut(),
        mLen: 0,
    };
    fetch_string(&mut left_str, left, vm);
    fetch_string(&mut right_str, right, vm);
    let len = left_str.mLen + right_str.mLen;
    if len <= 0 {
        release_fetch_string_temp(left_str, left, vm);
        release_fetch_string_temp(right_str, right, vm);
        return;
    }
    let creator = vm.mCreator;
    let concat = runtime_new_data_zeroed(creator, len as LInt) as *mut LInt8;
    if concat.is_null() {
        release_fetch_string_temp(left_str, left, vm);
        release_fetch_string_temp(right_str, right, vm);
        return;
    }
    ptr::copy_nonoverlapping(left_str.mPtr, concat, left_str.mLen as usize);
    ptr::copy_nonoverlapping(
        right_str.mPtr,
        concat.add(left_str.mLen as usize),
        right_str.mLen as usize,
    );
    release_fetch_string_temp(left_str, left, vm);
    release_fetch_string_temp(right_str, right, vm);

    let obj_body = create_string_object(concat, len, vm);
    if obj_body.is_null() {
        if !creator.is_null() {
            (*creator).delete_data(concat as *mut LVoid);
        }
        return;
    }
    (*right).mValueType = ValueType::BY_CLASS;
    (*right).mNameKey = BuiltinId::kBoyiaString.as_key();
    (*right).mValue.mObj.mPtr = obj_body as LIntPtr;
    (*right).mValue.mObj.mSuper = K_BOYIA_NULL;
}

/// CreateStringObject(LInt8* buffer, LInt len, LVoid* vm) per BoyiaValue.cpp.
/// CopyObject(kBoyiaString, 32, vm), set mParams[1].mStrVal, mParams[0].mIntVal = GenHashCode.
pub unsafe fn create_string_object(buffer: *mut LInt8, len: LInt, vm: &mut BoyiaVM) -> *mut BoyiaFunction {
    let key = BuiltinId::kBoyiaString.as_key();
    let obj = copy_object(key, 32, vm) as *mut BoyiaFunction;
    if obj.is_null() {
        return ptr::null_mut();
    }
    (*obj).mParams.add(1).write(BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_STRING,
        mValue: RealValue {
            mStrVal: BoyiaStr { mPtr: buffer, mLen: len },
        },
    });
    (*obj).mParams.add(0).write(BoyiaValue {
        mNameKey: 0,
        mValueType: ValueType::BY_INT,
        mValue: RealValue {
            mIntVal: gen_hash_code(buffer, len),
        },
    });
    obj
}

/// CreateConstString(BoyiaValue* value, LInt8* buffer, LInt len, LVoid* vm) per BoyiaValue.cpp.
/// CreateStringObject; fill value (BY_CLASS, mPtr, mSuper=K_BOYIA_NULL); mark objBody->mParamCount |= (kConstStringBuffer << 18).
/// Returns true on success (caller can return kOpResultEnd if false).
pub unsafe fn create_const_string(value: *mut BoyiaValue, buffer: *mut LInt8, len: LInt, vm: &mut BoyiaVM) -> bool {
    if value.is_null() {
        return false;
    }
    let obj_body = create_string_object(buffer, len, vm);
    if obj_body.is_null() {
        return false;
    }
    (*value).mValueType = ValueType::BY_CLASS;
    (*value).mValue.mObj.mPtr = obj_body as LIntPtr;
    (*value).mValue.mObj.mSuper = K_BOYIA_NULL;
    (*obj_body).mParamCount = (*obj_body).mParamCount | (K_CONST_STRING_BUFFER << K_STRING_BUFFER_SHIFT);
    true
}

/// CreateNativeString(BoyiaValue* value, LInt8* buffer, LInt len, LVoid* vm) per BoyiaValue.cpp.
/// CreateStringObject; fill value; mark objBody->mParamCount |= (kNativeStringBuffer << 18).
pub unsafe fn create_native_string(value: *mut BoyiaValue, buffer: *mut LInt8, len: LInt, vm: &mut BoyiaVM) {
    if value.is_null() {
        return;
    }
    let obj_body = create_string_object(buffer, len, vm);
    if obj_body.is_null() {
        println!("create_native_string obj_body is null");
        return;
    }
    (*value).mValueType = ValueType::BY_CLASS;
    (*value).mValue.mObj.mPtr = obj_body as LIntPtr;
    (*value).mValue.mObj.mSuper = K_BOYIA_NULL;
    (*obj_body).mParamCount = (*obj_body).mParamCount | (K_NATIVE_STRING_BUFFER << K_STRING_BUFFER_SHIFT);
}

// ---------------------------------------------------------------------------
// MicroTask helpers (internal)
// ---------------------------------------------------------------------------

unsafe fn add_micro_task(task: *mut MicroTask, vm: &mut BoyiaVM) {
    if task.is_null() {
        return;
    }
    let queue = vm.mTaskQueue;
    if queue.is_null() {
        return;
    }
    let list = &mut (*queue).mUsedTasks;
    (*task).mNext = ptr::null_mut();
    if !(*list).mHead.is_null() {
        (*(*list).mEnd).mNext = task;
        (*list).mEnd = task;
    } else {
        (*list).mHead = task;
        (*list).mEnd = task;
    }
}

unsafe fn alloc_micro_task_list_append(list: *mut MicroTaskList, task: *mut MicroTask) {
    if list.is_null() || task.is_null() {
        return;
    }
    (*task).mAllocLink.mLinkNext = ptr::null_mut();
    if !(*list).mHead.is_null() {
        (*task).mAllocLink.mLinkPrev = (*list).mEnd;
        (*(*list).mEnd).mAllocLink.mLinkNext = task;
        (*list).mEnd = task;
    } else {
        (*list).mHead = task;
        (*task).mAllocLink.mLinkPrev = ptr::null_mut();
        (*list).mEnd = task;
    }
}

/// Free a micro task (return to alloc list / dealloc). Used by exec_pop_function and consume_micro_task.
pub(crate) unsafe fn free_micro_task(task: *mut MicroTask, vm: &mut BoyiaVM) {
    if task.is_null() {
        return;
    }
    let queue = vm.mTaskQueue;
    if queue.is_null() {
        return;
    }
    let list = &mut (*queue).mAllocTasks;
    let prev = (*task).mAllocLink.mLinkPrev;
    let next = (*task).mAllocLink.mLinkNext;
    if !next.is_null() {
        (*next).mAllocLink.mLinkPrev = prev;
    } else {
        (*list).mEnd = prev;
    }
    if !prev.is_null() {
        (*prev).mAllocLink.mLinkNext = next;
    } else {
        (*list).mHead = next;
    }
    let cache = (*queue).mTaskCache;
    if !cache.is_null() {
        free_memory_chunk(task as *mut LVoid, cache);
    }
}

/// Match `AllocMicroTask` in BoyiaCore.cpp (`ALLOC_CHUNK(MicroTask, queue->mTaskCache)`).
pub(crate) unsafe fn alloc_micro_task(vm: &mut BoyiaVM) -> *mut MicroTask {
    let queue = vm.mTaskQueue;
    if queue.is_null() || (*queue).mTaskCache.is_null() {
        return ptr::null_mut();
    }
    let task = alloc_memory_chunk((*queue).mTaskCache) as *mut MicroTask;
    if task.is_null() {
        return ptr::null_mut();
    }
    (*task).mResult.mValueType = ValueType::BY_ARG;
    (*task).mObjRef.mValueType = ValueType::BY_ARG;
    (*task).mNext = ptr::null_mut();
    (*task).mAllocLink.mLinkNext = ptr::null_mut();
    alloc_micro_task_list_append(&mut (*queue).mAllocTasks, task);
    task
}

// ---------------------------------------------------------------------------
// MicroTask public API
// ---------------------------------------------------------------------------

/// Create micro task; value copied to mObjRef.
pub unsafe fn create_micro_task(vm: &mut BoyiaVM, value: *mut BoyiaValue) -> *mut LVoid {
    let task = alloc_micro_task(vm);
    if task.is_null() {
        return ptr::null_mut();
    }
    if !value.is_null() {
        value_copy_no_name(&mut (*task).mObjRef, value);
    }
    (*task).mAsyncEs = ptr::null_mut();
    task as *mut LVoid
}

/// Resume micro task: copy value to mResult, add to used queue.
pub unsafe fn resume_micro_task(
    task_ptr: *mut LVoid,
    value: *mut BoyiaValue,
    vm: &mut BoyiaVM,
) {
    if task_ptr.is_null() {
        return;
    }
    let task = task_ptr as *mut MicroTask;
    if !value.is_null() {
        value_copy_no_name(&mut (*task).mResult, value);
    }

    println!("call resume_micro_task");
    add_micro_task(task, vm);
}

/// Switch current exec state to the given one; updates vm's mEState, mLocals, mOpStack, mLoopStack, mExecStack.
pub(crate) unsafe fn switch_exec_state(exec_state: *mut ExecState, vm: &mut BoyiaVM) {
    if exec_state.is_null() {
        return;
    }
    vm.mEState = exec_state;
    vm.mLocals = (*exec_state).mLocals.as_mut_ptr();
    vm.mOpStack = (*exec_state).mOpStack.as_mut_ptr();
    vm.mLoopStack = (*exec_state).mLoopStack.as_mut_ptr();
    vm.mExecStack = (*exec_state).mExecStack.as_mut_ptr();
}

/// Unlink exec state from ESLink list and free it. If state had mTopTask, caller must free it first.
pub(crate) unsafe fn destroy_exec_state(state: *mut ExecState, vm: &mut BoyiaVM) {
    if state.is_null() {
        return;
    }
    let next = (*state).mNext;
    let prev = (*state).mPrev;
    if !next.is_null() {
        (*next).mPrev = prev;
        if !prev.is_null() {
            (*prev).mNext = next;
        }
    } else {
        if !prev.is_null() {
            (*prev).mNext = ptr::null_mut();
        }
        vm.mESLink = prev;
    }
    (*state).mTopTask = ptr::null_mut();
    (*state).mLast = ptr::null_mut();
    free_exec_state(state, vm);
}

/// Consume micro tasks. Strictly matches ConsumeMicroTask in BoyiaCore.cpp:
/// for each task in mUsedTasks: if aes && aes->mStackFrame.mPC then switch to aes, advance PC,
/// copy task->mResult to Reg0, ExecInstruction; if aes->mStackFrame.mContext is null then
/// copy Reg0 to mTopTask->mResult and AddMicroTask(mTopTask), then DestroyExecState(aes) when mLast->mWait;
/// switch back; unlink task and FreeMicroTask.
pub unsafe fn consume_micro_task(vm: &mut BoyiaVM) {
    let queue_ptr = vm.mTaskQueue;
    if queue_ptr.is_null() {
        return;
    }
    let queue = &mut *queue_ptr;
    let mut task = (*queue).mUsedTasks.mHead;
    while !task.is_null() {
        let aes = (*task).mAsyncEs;
        if !aes.is_null() && crate::execute::pc_is_valid((*aes).mStackFrame.mPC) {
            let current_state = vm.mEState;
            switch_exec_state(aes, vm);
            (*aes).mWait = LFalse;
            (*aes).mStackFrame.mPC =
                crate::execute::next_pc((*aes).mStackFrame.mPC, vm);
            value_copy(&mut vm.mCpu.mReg0, &(*task).mResult);

            crate::execute::exec_instruction(vm);
            if (*aes).mStackFrame.mContext.is_null() {
                if !(*aes).mTopTask.is_null() {
                    value_copy(&mut (*(*aes).mTopTask).mResult, &vm.mCpu.mReg0);

                    add_micro_task((*aes).mTopTask, vm);
                }
                if !(*aes).mLast.is_null() && (*(*aes).mLast).mWait != LFalse {
                    destroy_exec_state(aes, vm);
                }
            }
            switch_exec_state(current_state, vm);
        }
        let tmp = task;
        task = (*tmp).mNext;
        (*queue).mUsedTasks.mHead = task;
        free_micro_task(tmp, vm);
    }
}

/// Iterate micro tasks (alloc list).
/// `task == null` starts from the queue head; otherwise `task` points at the current [MicroTask].
pub unsafe fn iterate_micro_task(
    obj: *mut *mut BoyiaValue,
    result: *mut *mut BoyiaValue,
    vm: &mut BoyiaVM,
    task: *mut LVoid,
) -> *mut LVoid {
    if !result.is_null() {
        *result = ptr::null_mut();
    }
    if !obj.is_null() {
        *obj = ptr::null_mut();
    }
    let queue = vm.mTaskQueue;
    if queue.is_null() {
        return ptr::null_mut();
    }
    let task = if task.is_null() {
        (*queue).mAllocTasks.mHead
    } else {
        task as *mut MicroTask
    };
    if task.is_null() {
        return task as *mut LVoid;
    }
    if !result.is_null() {
        *result = &mut (*task).mResult;
    }
    if !obj.is_null() {
        *obj = &mut (*task).mObjRef;
    }
    (*task).mAllocLink.mLinkNext as *mut LVoid
}

pub(crate) unsafe fn alloc_object<T>() -> *mut T {
    let size = std::mem::size_of::<T>();
    let ptr = crate::fast_malloc(size as LInt) as *mut T;
    if ptr.is_null() {
        return ptr::null_mut();
    }
    ptr::write_bytes(ptr as *mut u8, 0, size);
    ptr
}
 