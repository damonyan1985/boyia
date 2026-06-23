//! BoyiaRuntime: VM lifecycle, native function table, init and execution.
//! Rust port of BoyiaRuntime.cpp (without platform/UI/GC; stubs where needed).

#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use crate::id_creator::IdCreator;
use crate::info::BoyiaCompileInfo;
use boyia_builtins::{
    builtin_array_class, builtin_map_class, builtin_micro_task_class, builtin_string_class,
};
use boyia_vm::{
    cache_vm_code, consume_micro_task, delete_data, execute_global_code,
    free_memory_pool, get_runtime_from_vm, init_memory_pool, init_vm_boxed, new_data, vm_from_void,
    CompileArgs, BoyiaFunction, BoyiaStr, BoyiaVM, BoyiaValue, CompileFunction, CompileNativeFunction,
    Global, GlobalList, K_BOYIA_NULL, LInt, LUintPtr, LVoid, NativeFunction, NativePtr,
    OpHandleResult, OpOffset, Runtime, ValueType,
};
use std::any::Any;
use std::ptr;

const K_NATIVE_FUNCTION_CAPACITY: usize = 100;
const K_COMPILE_FUNCTION_CAPACITY: usize = 32;
/// Memory pool size (6 MB). Match BoyiaRuntime.cpp kMemoryPoolSize.
const K_MEMORY_POOL_SIZE: LInt = 6 * 1024 * 1024;

/// Runtime state: VM + native table + id creator + memory pool + GC. Matches BoyiaRuntime.cpp.
pub struct BoyiaRuntime {
    /// VM instance (creator set to self for native dispatch).
    vm: Option<Box<BoyiaVM>>,
    /// Memory pool for object allocation (BoyiaRuntime::m_memoryPool). Created in init(); freed in Drop after VM drop.
    memory_pool: *mut LVoid,
    /// GC state (BoyiaRuntime::m_gc). Created in init() after VM; destroyed in Drop before VM drop.
    gc: *mut boyia_gc::BoyiaGc,
    /// Native function table: (name_key, ptr). Terminated by mAddr == null (we use 0 index as sentinel or check length).
    native_fun_table: Vec<NativeFunction>,
    /// Compile-time function table (e.g. `require`), dispatched while compiling.
    compile_fun_table: Vec<CompileNativeFunction>,
    id_creator: IdCreator,
    /// C++ `m_compileInfo` (`BoyiaCompileInfo`).
    compile_info: BoyiaCompileInfo,
    /// Whether VM code was loaded from exe/cache bundle (C++ `m_isLoadExeFile`); `BY_Require` no-ops when true.
    is_load_exe_file: bool,
    /// Persistent BoyiaValue list; keeps references so objects are not collected.
    persistent_objects: GlobalList,
    /// Embedder-owned data (e.g. CLI [AsyncCtx]). Not part of C++ port.
    embedder: Option<Box<dyn Any + Send + Sync>>,
}

impl BoyiaRuntime {
    /// Create runtime; VM is created in `init()` with `self` as creator. No global dispatchers.
    /// After `init()`, the runtime must not be moved (e.g. use `Box<BoyiaRuntime>`) so that the VM's mCreator stays valid.
    fn new() -> Self {
        Self {
            vm: None,
            memory_pool: ptr::null_mut(),
            gc: ptr::null_mut(),
            native_fun_table: Vec::with_capacity(K_NATIVE_FUNCTION_CAPACITY),
            compile_fun_table: Vec::with_capacity(K_COMPILE_FUNCTION_CAPACITY),
            id_creator: IdCreator::new(),
            compile_info: BoyiaCompileInfo::new(),
            is_load_exe_file: false,
            persistent_objects: GlobalList::new(),
            embedder: None,
        }
    }

    /// Store embedder data keyed by type (replaces any previous embedder).
    pub fn set_embedder<T: Any + Send + Sync>(&mut self, data: T) {
        self.embedder = Some(Box::new(data));
    }

    /// Borrow embedder data if the type matches.
    pub fn embedder<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.embedder.as_ref()?.downcast_ref()
    }

    /// Mutably borrow embedder data if the type matches.
    pub fn embedder_mut<T: Any + Send + Sync>(&mut self) -> Option<&mut T> {
        self.embedder.as_mut()?.downcast_mut()
    }

    /// Initialize: create memory pool (BoyiaMemory), then VM with `self` as creator (like C++ BoyiaRuntime ctor). Call after new().
    fn init(&mut self) {
        eprintln!("[init] 1 memory pool");
        self.memory_pool = unsafe { init_memory_pool(K_MEMORY_POOL_SIZE) };
        if self.memory_pool.is_null() {
            eprintln!("[init] ERROR init_memory_pool returned null");
            return;
        }
        eprintln!("[init] 2 init_vm");
        self.vm = unsafe { init_vm_boxed(self as &mut dyn Runtime as *mut dyn Runtime) };
        if self.vm.is_none() {
            eprintln!("[init] ERROR init_vm returned null");
            return;
        }
        eprintln!("[init] 2a create_gc");
        self.gc = unsafe { boyia_gc::create_gc() };
        if self.gc.is_null() {
            eprintln!("[init] WARN create_gc returned null");
        }
        eprintln!("[init] 2 builtin ids (BuiltinId 1..6 reserved)");
        // Ensure builtin names are registered so compile/lookup use same keys as CreateGlobalClass
        let _ = self.id_creator.gen_ident_by_str("this");
        let _ = self.id_creator.gen_ident_by_str("super");
        let _ = self.id_creator.gen_ident_by_str("String");
        let _ = self.id_creator.gen_ident_by_str("Array");
        let _ = self.id_creator.gen_ident_by_str("Map");
        let _ = self.id_creator.gen_ident_by_str("MicroTask");

        eprintln!("[init] 3 init_native_function");
        self.init_native_function();
        self.init_compile_function();

        eprintln!("[init] 4 builtin_string_class");
        // Builtin classes: use BuiltinId keys per BoyiaValue.h (CreateGlobalClass(kBoyiaString, vm) etc.)
        let BoyiaRuntime {
            id_creator,
            vm,
            ..
        } = self;
        let vm = vm.as_deref_mut().expect("vm initialized");
        let mut gen_id = |s: &str| id_creator.gen_ident_by_str(s);
        builtin_string_class(vm, &mut gen_id);
        eprintln!("[init] 5 builtin_map_class");
        builtin_map_class(vm, &mut gen_id);
        eprintln!("[init] 6 builtin_micro_task_class");
        builtin_micro_task_class(vm, &mut gen_id);
        eprintln!("[init] 7 builtin_array_class");
        builtin_array_class(vm, &mut gen_id);

        eprintln!("[init] 8 done");
    }

    /// Create and initialize a runtime in one step on a stable heap address.
    /// Equivalent to `Box::new(BoyiaRuntime::new())` + `init()`.
    pub fn create() -> Box<Self> {
        let mut runtime = Box::new(Self::new());
        runtime.init();
        runtime
    }

    /// Create a runtime with minimal initialization for tests.
    #[doc(hidden)]
    pub fn create_minimal_for_test() -> Box<Self> {
        let mut runtime = Box::new(Self::new());
        runtime.init_minimal_for_test();
        runtime
    }

    /// Minimal init for tests: VM + natives + dispatcher only (no builtin classes).
    #[doc(hidden)]
    pub fn init_minimal_for_test(&mut self) {
        self.memory_pool = unsafe { init_memory_pool(K_MEMORY_POOL_SIZE) };
        if self.memory_pool.is_null() {
            return;
        }
        self.vm = unsafe { init_vm_boxed(self as &mut dyn Runtime as *mut dyn Runtime) };
        if self.vm.is_none() {
            return;
        }
        self.gc = unsafe { boyia_gc::create_gc() };
        self.id_creator.gen_ident_by_str("this");
        self.id_creator.gen_ident_by_str("String");
        self.init_native_function();
        self.init_compile_function();
    }

    fn init_native_function(&mut self) {
        self.append_native("new", boyia_lib::create_object as NativePtr);
        self.append_native("BY_Log", boyia_lib::log_print as NativePtr);
        self.append_native("require", boyia_lib::require_file as NativePtr);
        self.append_native_sentinel();
    }

    /// Register compile-time functions (resolved while compiling, before native dispatch).
    fn init_compile_function(&mut self) {
        self.append_compile_native("require", boyia_lib::require_file_compile as CompileFunction);
        self.append_compile_native_sentinel();
    }

    fn append_native(&mut self, name: &str, ptr: NativePtr) {
        let id = self.id_creator.gen_ident_by_str(name);
        if self.native_fun_table.len() < self.native_fun_table.capacity() {
            self.native_fun_table.push(NativeFunction {
                mNameKey: id,
                mAddr: ptr,
            });
        }
    }

    fn append_native_sentinel(&mut self) {
        self.native_fun_table.push(NativeFunction {
            mNameKey: 0,
            mAddr: sentinel_native as NativePtr,
        });
    }

    fn append_compile_native(&mut self, name: &str, ptr: CompileFunction) {
        let id = self.id_creator.gen_ident_by_str(name);
        if self.compile_fun_table.len() < self.compile_fun_table.capacity() {
            self.compile_fun_table.push(CompileNativeFunction {
                mNameKey: id,
                mAddr: ptr,
            });
        }
    }

    fn append_compile_native_sentinel(&mut self) {
        self.compile_fun_table.push(CompileNativeFunction {
            mNameKey: 0,
            mAddr: sentinel_compile_native as CompileFunction,
        });
    }

    /// Compile script source into the VM (`BoyiaCompileInfo::compile` / `CompileCode`).
    /// Compile-time `require` is resolved via a post-order DFS: dependencies compile before the
    /// entry, and a module's own dependencies compile before it (children-first execution order).
    pub fn compile(&mut self, script: &str) {
        let BoyiaRuntime {
            compile_info,
            id_creator,
            vm,
            ..
        } = self;
        if let Some(vm) = vm.as_deref_mut() {
            compile_info.compile_entry(script, vm, id_creator);
        }
    }

    /// Set the entry script path used to resolve relative `require` at runtime (e.g. CLI `main.boyia`).
    pub fn set_entry_script_path(&mut self, path: &str) {
        self.compile_info.set_entry_script_path(path);
    }

    /// C++ `BoyiaRuntime::compileFile` → `m_compileInfo->compileFile(path)`.
    pub fn compile_file(&mut self, path: &str) {
        let BoyiaRuntime {
            compile_info,
            id_creator,
            vm,
            ..
        } = self;
        if let Some(vm) = vm.as_deref_mut() {
            compile_info.compile_file(path, vm, id_creator);
        }
    }

    fn vm_mut(&mut self) -> Option<&mut BoyiaVM> {
        self.vm.as_deref_mut()
    }

    fn vm_ptr(&self) -> *mut LVoid {
        self.vm
            .as_ref()
            .map(|vm| vm.as_ref() as *const BoyiaVM as *mut LVoid)
            .unwrap_or(ptr::null_mut())
    }

    /// VM pointer for legacy GC / memory boundaries.
    pub fn vm(&self) -> *mut LVoid {
        self.vm_ptr()
    }

    /// Id creator for string keys.
    pub fn id_creator(&mut self) -> &mut IdCreator {
        &mut self.id_creator
    }

    /// Borrow VM and id creator together (disjoint fields) for embedder builtin registration.
    pub fn with_vm_and_id_creator<R>(
        &mut self,
        f: impl FnOnce(&mut BoyiaVM, &mut IdCreator) -> R,
    ) -> Option<R> {
        let BoyiaRuntime { id_creator, vm, .. } = self;
        vm.as_deref_mut().map(|vm| f(vm, id_creator))
    }

    pub fn is_load_exe_file(&self) -> bool {
        self.is_load_exe_file
    }

    /// Run global code (entry table). Match ExecuteGlobalCode.
    pub fn run_exe_file(&mut self) {
        if let Some(vm) = self.vm_mut() {
            unsafe {
                execute_global_code(vm);
            }
        }
    }

    /// Cache VM code (patch instructions). Match CacheVMCode.
    pub fn cache_code(&mut self) {
        if let Some(vm) = self.vm_mut() {
            unsafe {
                cache_vm_code(vm);
            }
        }
    }

    /// Consume micro tasks in the queue.
    pub fn consume_micro_task(&mut self) {
        if let Some(vm) = self.vm_mut() {
            unsafe {
                consume_micro_task(vm);
            }
        }
    }
}

/// Recover [BoyiaRuntime] from a VM when the creator is [BoyiaRuntime] (CLI / full runtime).
pub unsafe fn boyia_runtime_from_vm(vm: &mut BoyiaVM) -> Option<&mut BoyiaRuntime> {
    let rt = get_runtime_from_vm(vm);
    if rt.is_null() {
        return None;
    }
    Some(&mut *(rt as *mut BoyiaRuntime))
}

impl Runtime for BoyiaRuntime {
    fn memory_pool(&self) -> *mut LVoid {
        self.memory_pool
    }

    fn vm_ptr(&self) -> *mut LVoid {
        BoyiaRuntime::vm_ptr(self)
    }

    fn gc_append_ref(&self, address: *mut LVoid, type_: boyia_vm::ValueType) {
        boyia_gc::gc_append_ref(address, type_, self.gc);
    }

    fn create_runtime_to_memory(&self, _vm: &mut BoyiaVM) -> *mut LVoid {
        unsafe { boyia_vm::init_memory_pool(K_MEMORY_POOL_SIZE) }
    }

    fn update_runtime_memory(&mut self, to_pool: *mut LVoid, _vm: &mut BoyiaVM) {
        if to_pool.is_null() {
            return;
        }
        unsafe {
            if !self.memory_pool.is_null() {
                boyia_vm::free_memory_pool(self.memory_pool);
            }
            self.memory_pool = to_pool;
        }
    }

    fn find_native_func(&self, key: LUintPtr) -> LInt {
        for (idx, nf) in self.native_fun_table.iter().enumerate() {
            if nf.mNameKey == 0 || nf.mAddr as *const () == sentinel_native as *const () {
                break;
            }
            if nf.mNameKey == key {
                return idx as LInt;
            }
        }
        -1
    }

    fn call_native_function(&mut self, vm: &mut BoyiaVM, idx: LInt) -> LInt {
        if idx < 0 || idx as usize >= self.native_fun_table.len() {
            return OpHandleResult::kOpResultEnd as i32;
        }
        let nf = &self.native_fun_table[idx as usize];
        if nf.mAddr as *const () == sentinel_native as *const () {
            return OpHandleResult::kOpResultEnd as i32;
        }
        unsafe { (nf.mAddr)(vm) as i32 }
    }

    fn find_compile_func(&self, key: LUintPtr) -> LInt {
        for (idx, cf) in self.compile_fun_table.iter().enumerate() {
            if cf.mNameKey == 0 || cf.mAddr as *const () == sentinel_compile_native as *const () {
                break;
            }
            if cf.mNameKey == key {
                return idx as LInt;
            }
        }
        -1
    }

    fn call_compile_function(&self, idx: LInt, args: &CompileArgs) -> bool {
        if idx < 0 || idx as usize >= self.compile_fun_table.len() {
            return false;
        }
        let cf = &self.compile_fun_table[idx as usize];
        if cf.mAddr as *const () == sentinel_compile_native as *const () {
            return false;
        }
        unsafe { (cf.mAddr)(args) }
    }

    fn enqueue_compile_script(&mut self, resolved_path: &str) {
        self.compile_info.enqueue_script(resolved_path);
    }

    fn gen_identifier(&mut self, key: &str) -> LUintPtr {
        self.id_creator.gen_ident_by_str(key)
    }

    fn gen_ident_by_str(&mut self, s: *const BoyiaStr) -> LUintPtr {
        self.id_creator.gen_ident_by_boyia_str(s)
    }

    fn name_for_identifier(&self, id: LUintPtr) -> Option<String> {
        self.id_creator.name_for_ident(id)
    }

    fn new_data(&self, size: LInt) -> *mut LVoid {
        unsafe {
            if !self.gc.is_null() {
                if let Some(vm) = vm_from_void(self.vm_ptr()) {
                    boyia_gc::gc_collect_garbage(self.gc, vm);
                }
            }
            new_data(size, self.memory_pool)
        }
    }

    fn delete_data(&self, data: *mut LVoid) {
        unsafe { delete_data(data, self.memory_pool) }
    }

    fn persistent_object(&mut self, value: *const BoyiaValue) -> *mut Global {
        if value.is_null() {
            return std::ptr::null_mut();
        }
        self.persistent_objects.push_back(unsafe { *value })
    }

    fn iterate_persistent(&mut self, f: &mut dyn FnMut(*mut BoyiaValue)) {
        let mut ptr = self.persistent_objects.head();
        while !ptr.is_null() {
            let next = unsafe { (*ptr).next() };
            let vp = unsafe { (*ptr).value_ptr() };
            unsafe {
                if (*vp).mValueType == ValueType::BY_ANONYM_FUNC {
                    let mptr = (*vp).mValue.mObj.mPtr;
                    if mptr == K_BOYIA_NULL {
                        self.persistent_objects.remove(ptr);
                        ptr = next;
                        continue;
                    }
                    let fun = mptr as *mut BoyiaFunction;
                    if !fun.is_null()
                        && !(*fun).mParams.is_null()
                        && (*fun).mCaptureCount > 0
                    {
                        let base = (*fun).mParams.offset((*fun).mParamSize as isize);
                        for i in 0..(*fun).mCaptureCount as isize {
                            f(base.offset(i));
                        }
                    }
                    ptr = next;
                    continue;
                }
            }
            f(vp);
            ptr = next;
        }
    }

    fn remove_persistent(&mut self, ptr: *mut Global) {
        self.persistent_objects.remove(ptr);
    }

    fn is_load_exe_file(&self) -> bool {
        self.is_load_exe_file
    }

    fn require_path_base(&self) -> &str {
        self.compile_info.require_path_base()
    }

    fn compile_script_file(&mut self, resolved_path: &str) {
        let BoyiaRuntime {
            compile_info,
            id_creator,
            vm,
            ..
        } = self;
        if let Some(vm) = vm.as_deref_mut() {
            compile_info.compile_file(resolved_path, vm, id_creator);
        }
    }

    fn set_code_position(&mut self, code_index: OpOffset, line_num: LInt, column_num: LInt) {
        self.compile_info
            .set_code_position(code_index, line_num, column_num);
    }
}

impl Default for BoyiaRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BoyiaRuntime {
    fn drop(&mut self) {
        unsafe {
            if !self.gc.is_null() {
                boyia_gc::destroy_gc(self.gc);
                self.gc = ptr::null_mut();
            }
            drop(self.vm.take());
            if !self.memory_pool.is_null() {
                free_memory_pool(self.memory_pool);
                self.memory_pool = ptr::null_mut();
            }
        }
    }
}

/// Sentinel: end of native table (never called with valid idx).
unsafe fn sentinel_native(_vm: &mut BoyiaVM) -> OpHandleResult {
    OpHandleResult::kOpResultEnd
}

/// Sentinel: end of compile-time function table.
unsafe fn sentinel_compile_native(_args: &CompileArgs) -> bool {
    false
}
