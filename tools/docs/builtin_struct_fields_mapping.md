# Builtin Native 对象映射（`#[boyia_native_object]`）

本文以 `examples/boyia_cli/src/builtins/external/config.rs` 中的 **Config** 内置类为例，说明 Rust struct 如何通过 `nativePtr` + `Box<T>` 挂到 Boyia 实例上，以及 `#[boyia_native_object]` / `#[boyia_class(...)]` 的完整使用与宏展开流程。

相关总览见 [Boyia 语言开发文档](./boyia_language_development.md) 第 6 节。  
若关注 `onReceive` 一类持续回调的宏化实现，参见 [Persistent Callback 宏改造方案](./persistent_callback_macro_design.md)。

> **说明：** 旧版 `#[boyia_fields]`（把字段镜像到 VM `mParams`、每次调用 `load`/`store`）已移除。带 `self` 的 builtin 类统一使用本文描述的 **native object** 方案。

---

## 1. 源码：你写什么

```rust
// examples/boyia_cli/src/builtins/external/config.rs

use builtin_macro::{boyia_class, boyia_native_object};

#[boyia_native_object]
pub struct ConfigBuiltins {
    #[boyia_field_default = "false"]
    debug: bool,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(method = "getDebug")]
    fn get_debug(&self) -> bool {
        self.debug
    }

    #[boyia_sync_builtin(method = "setDebug")]
    fn set_debug(&mut self, value: bool) {
        self.debug = value;
    }

    #[boyia_sync_builtin(method = "getTimeout")]
    fn get_timeout(&self) -> u64 {
        self.timeout_ms * 2
    }

    #[boyia_sync_builtin(method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
}
```

宏分工：

| 宏 | 挂在哪 | 作用 |
|----|--------|------|
| `#[boyia_native_object]` | `struct ConfigBuiltins` | 注入 `__boyia_hdr`、实现 `NativePropTrait`，生成 `boyia_default()` |
| `#[boyia_field_default = "..."]` | 字段上 | `Box` 首次分配时的字段初值 |
| `#[boyia_class(name = "Config", registrar = ...)]` | `impl` 上 | 注册 Boyia 类与方法；**若存在带 `self` 的 sync 方法**，宏要求 struct 已实现 `NativePropTrait`（即已加 `#[boyia_native_object]`），并自动挂 `nativePtr`、走 `boyia_native_ref` / `boyia_native_mut` |
| `#[boyia_sync_builtin(method = "...")]` | 方法上 | 把 Rust 方法映射为脚本可调用的同步 native 方法；native 符号默认 `{方法名}_native` |

### 1.1 `#[boyia_field_default]` 支持的字段类型

属性写法统一为 **字符串字面量**：`#[boyia_field_default = "..."]`。宏按字段的 Rust 类型解析引号内的文本。

| Rust 字段类型 | `boyia_field_default` 示例 | 省略时的默认值 | 说明 |
|---------------|------------------------------|----------------|------|
| `bool` | `#[boyia_field_default = "true"]` | `false` | 仅 `"true"` 为 true，其余为 false |
| `String` | `#[boyia_field_default = "production"]` | `""`（空字符串） | 写入 `Box` 初值 |
| `f32` / `f64` | `#[boyia_field_default = "1.5"]` | `0.0` | 按浮点解析 |
| `u64` / `usize` | `#[boyia_field_default = "30000"]` | `0` | 按无符号整数解析 |
| `i8` / `i16` / `i32` / `i64` / `isize` | `#[boyia_field_default = "-1"]` | `0` | 按有符号整数解析 |
| `u8` / `u16` / `u32` | `#[boyia_field_default = "255"]` | `0` | 按有符号解析后转型 |

未列出的类型（如 `Option<T>`、`Vec<T>`、自定义 struct）暂不支持，编译期会报错。

### 1.2 `#[boyia_class]` 与 `#[boyia_native_object]` 如何配合

**不再需要**在 `#[boyia_class]` 上写 `native` 标志（已移除；写上会编译报错）。

| 场景 | struct | impl 方法 | 宏行为 |
|------|--------|-----------|--------|
| 纯静态 API（`File`、`Json`、`OS`） | 空 struct 或普通 struct | 无 `self` | 只注册方法，不挂 `nativePtr` |
| 带实例状态（`Config`） | `#[boyia_native_object]` | `&self` / `&mut self` | 编译期检查 `NativePropTrait`；注册时 `attach_native_ptr_slot`；handler 用 `boyia_native_ref` / `mut` |

规则：**只要 sync 方法带 `self`，就必须在对应 struct 上使用 `#[boyia_native_object]`**；二者缺一会在编译期失败。

---

## 2. 注册流程：从编译到 VM 里有 `Config` 类

```
cargo build
    │
    ▼
过程宏展开 config.rs（见第 5、6 节）
    │
    ▼
生成 builtin_config_class 函数
    │
    ▼
builtins/mod.rs 把它放进 DEFAULT_BUILTINS
    │
    ▼
BoyiaRunner::create(registrars) 在 Boyia 任务线程依次调用 registrar
    │
    ▼
register_async_builtin_class(vm, gen_id, "Config", |class_body, ...| {
    attach_native_ptr_slot(class_body, gen_id)   // 挂 nativePtr 槽（index 0）
    attach_method(..., "getDebug",  ...)
    attach_method(..., "setDebug",  ...)
    attach_method(..., "getTimeout", ...)
    attach_method(..., "setTimeout", ...)
})
```

注册完成后，VM 里存在全局类 **Config**：

- **内部槽位**：`nativePtr`（`mParams[0]`，`BY_NAVCLASS`，初值 0）
- **方法**（来自 `#[boyia_sync_builtin]`）：`getDebug`、`setDebug`、`getTimeout`、`setTimeout`

Rust 字段（`debug`、`timeout_ms`）**不会**注册为脚本可访问的 Boyia 属性；脚本只能通过方法读写。

注册表入口：

```rust
// examples/boyia_cli/src/builtins/mod.rs
pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    external::config::builtin_config_class,
    // ...
];
```

---

## 3. 脚本使用流程

`examples/boyia_cli/script/main.boyia` 中的用法：

```boyia
var config = new(Config);
config.setTimeout(5333);
Util.log("config.getTimeout() : " + config.getTimeout());
```

```
new(Config)
  → VM 从 Config 类模板 copy 出实例
  → 实例 mParams[0]（nativePtr）= 0，尚未分配 Rust Box

config.setTimeout(5333)
  → 调用 native config_set_timeout_native
  → handler：boyia_native_mut → 首次分配 Box<ConfigBuiltins>（默认值 debug=false, timeout_ms=30000）
  → set_timeout 写入 timeout_ms = 5333（直接改堆上 Box，无 load/store）

config.getTimeout()
  → handler：boyia_native_ref → 复用已有 Box
  → get_timeout 返回 timeout_ms * 2
  → 日志打印 10666（5333 * 2）
```

说明：

- **方法**（`setTimeout` / `getTimeout`）是脚本唯一入口。
- **字段**（`debug`、`timeout_ms`）只存在于 Rust `Box` 内，脚本不能写 `config.timeout_ms`。
- 实例在**第一次**带 `self` 的 native 调用时才懒分配 `Box`；GC 通过 `nativePtr` + vtable 跟踪这块堆内存。

---

## 4. 单次方法调用的运行时流程

以 `config.setTimeout(5333)` 为例：

```
┌─────────────────────────────────────────────────────────────┐
│ Boyia 脚本：config.setTimeout(5333)                          │
└────────────────────────────┬────────────────────────────────┘
                             ▼
┌─────────────────────────────────────────────────────────────┐
│ VM：BY_NAV_FUNC 调用前 local_push(this)                      │
│   this = config 实例对应的 BoyiaFunction                     │
└────────────────────────────┬────────────────────────────────┘
                             ▼
┌─────────────────────────────────────────────────────────────┐
│ config_set_timeout_native(vm)                                │
│   → set_timeout_handler(&mut SyncCallSite)                   │
└────────────────────────────┬────────────────────────────────┘
                             ▼
  ① class_body = site.this_function()     // 拿到 this（实例 BoyiaFunction）
  ② rt = get_runtime_from_vm(site.vm())
  ③ state = boyia_native_mut::<ConfigBuiltins>(class_body, rt)
       // nativePtr 为 0 时：Box::new(boyia_default())，写入 nativePtr，gc_append_ref
       // 已有 Box 时：直接返回 &mut T
  ④ ms = site.arg_i64(1) as u64           // 从 VM 读脚本参数 5333
  ⑤ ConfigBuiltins::set_timeout(state, ms)
       // 直接改堆上 Box：state.timeout_ms = ms
  ⑥ set_sync_return((), vm)               // 返回给脚本（无 store 回 mParams）
                             ▼
┌─────────────────────────────────────────────────────────────┐
│ Box<ConfigBuiltins>.timeout_ms == 5333                       │
│ nativePtr 仍指向同一块堆内存                                  │
└─────────────────────────────────────────────────────────────┘
```

`get_timeout`（`&self`）流程相同，但第 ③ 步用 `boyia_native_ref`（只读引用），且**无写回步骤**——状态本来就在 `Box` 里。

---

## 5. `#[boyia_native_object]` 宏展开流程

**输入**（你写的 struct）：

```rust
#[boyia_native_object]
pub struct ConfigBuiltins {
    #[boyia_field_default = "false"]
    debug: bool,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}
```

**宏在编译期做的事：**

1. 解析每个字段的名字、类型、`#[boyia_field_default]`
2. 给 struct 加 `#[repr(C)]`，并在**首位**插入 `__boyia_hdr: NativePropHeader`
3. 生成 `NativePropTrait` 实现（vtable、`boyia_default`、`native_drop`）

**展开结果（概念代码，省略属性清理）：**

```rust
#[repr(C)]
pub struct ConfigBuiltins {
    __boyia_hdr: boyia_gc::NativePropHeader,
    debug: bool,
    timeout_ms: u64,
}

impl ConfigBuiltins {
    fn boyia_new_header() -> boyia_gc::NativePropHeader {
        boyia_gc::NativePropHeader::new(&<Self as boyia_gc::NativePropTrait>::VTABLE)
    }
}

impl boyia_gc::NativePropTrait for ConfigBuiltins {
    const VTABLE: boyia_gc::NativePropVTable = boyia_gc::NativePropVTable {
        mark_fn: Self::native_mark,
        flag_fn: Self::native_flag,
        drop_fn: Self::native_drop,
    };

    fn boyia_default() -> Self {
        Self {
            __boyia_hdr: Self::boyia_new_header(),
            debug: false,
            timeout_ms: 30000,
        }
    }

    unsafe fn native_drop(ptr: *mut LVoid) {
        let _ = Box::from_raw(ptr as *mut Self);
    }
}
```

要点：

- **持久状态在 Rust 堆上的 `Box<T>`**，通过实例 `mParams[0]`（`nativePtr`）关联。
- **懒分配**：`boyia_ensure_native` 在首次 `ref`/`mut` 时 `Box::new(T::boyia_default())`。
- **GC**：`gc_append_ref` 登记指针；回收时 vtable `drop_fn` 释放 `Box`。
- 宏**不会改写**你方法体里的 `self.timeout_ms`，那就是普通 Rust 字段访问。

---

## 6. `#[boyia_class(...)]` 宏展开流程

**输入**（你写的 impl）：

```rust
#[boyia_class(name = "Config", registrar = builtin_config_class)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
    // ... 其余方法同理
}
```

**宏在编译期对每个 `#[boyia_sync_builtin]` 方法：**

1. 识别 `&self` / `&mut self` / 无 self
2. 若存在任意带 `self` 的方法：生成 `NativePropTrait` 编译期断言，并在 registrar 里通过 trait 约束调用 `attach_native_ptr_slot`
3. 保留方法体，重新输出到 `impl ConfigBuiltins { ... }`
4. 生成 `{方法名}_handler` 与 `{方法名}_native`（`define_sync_native!`，符号默认为 `{Rust 方法名}_native`）
5. 生成 `builtin_config_class` 注册函数

**以 `set_timeout` 为例，展开出：**

```rust
// A. 保留你的 impl
impl ConfigBuiltins {
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
    // get_debug, set_debug, get_timeout ...
}

// B. 生成的 handler（VM 实际入口）
fn set_timeout_handler(site: &mut SyncCallSite<'_>) -> OpHandleResult {
    let class_body = some_or_end!(site.this_function());
    let rt = unsafe { get_runtime_from_vm(site.vm()) };
    if rt.is_null() {
        return OpHandleResult::kOpResultEnd;
    }
    let state = unsafe {
        boyia_gc::boyia_native_mut::<ConfigBuiltins>(class_body, &mut *rt)
    };

    let ms = some_or_end!(site.arg_i64(1)) as u64;

    let __sync_result = ConfigBuiltins::set_timeout(state, ms);

    crate::runner::sync::set_sync_return(__sync_result, site.vm())
}

// C. 注册为 native 函数
unsafe fn config_set_timeout_native(vm: &mut BoyiaVM) -> OpHandleResult {
    sync_dispatch(vm, 2, set_timeout_handler)  // min_locals = 参数数 + 2（含 this）
}

// D. 类注册器
pub fn builtin_config_class(
    vm: &mut BoyiaVM,
    gen_id: &mut dyn FnMut(&str) -> LUintPtr,
) {
    register_async_builtin_class(vm, gen_id, "Config", |class_body, vm, gen_id| {
        __boyia_attach_native_ptr::<ConfigBuiltins>(class_body, gen_id);
        attach_method(gen_id, "getDebug",    config_get_debug_native,    class_body, vm);
        attach_method(gen_id, "setDebug",    config_set_debug_native,    class_body, vm);
        attach_method(gen_id, "getTimeout",  config_get_timeout_native,  class_body, vm);
        attach_method(gen_id, "setTimeout",  config_set_timeout_native,  class_body, vm);
    });
}
```

**`get_timeout`（`&self`）的 handler 差异：**

```rust
fn get_timeout_handler(site: &mut SyncCallSite<'_>) -> OpHandleResult {
    let class_body = some_or_end!(site.this_function());
    let rt = unsafe { get_runtime_from_vm(site.vm()) };
    if rt.is_null() {
        return OpHandleResult::kOpResultEnd;
    }
    let state = unsafe {
        boyia_gc::boyia_native_ref::<ConfigBuiltins>(class_body, &mut *rt)
    };

    let __sync_result = ConfigBuiltins::get_timeout(state);

    set_sync_return(__sync_result, site.vm())
}
```

`ConfigBuiltins::set_timeout(state, ms)` 与 `state.set_timeout(ms)` 完全等价；宏选用关联函数写法，因为 handler 里已有名为 `state` 的局部变量。

---

## 7. 字段 vs 方法：分别映射到什么

| Rust 侧 | Boyia / 运行时侧 | 脚本示例 |
|---------|------------------|----------|
| 字段 `debug` | `Box` 内字段（脚本不可见） | 无；用 `config.getDebug()` |
| 字段 `timeout_ms` | `Box` 内字段（脚本不可见） | 无；用 `config.getTimeout()` / `setTimeout` |
| 方法 `get_debug` | 方法 `getDebug` | `config.getDebug()` |
| 方法 `set_debug` | 方法 `setDebug` | `config.setDebug(true)` |
| 方法 `get_timeout` | 方法 `getTimeout` | `config.getTimeout()` → 返回 `timeout_ms * 2` |
| 方法 `set_timeout` | 方法 `setTimeout` | `config.setTimeout(5333)` |
| （内部）`nativePtr` | `mParams[0]`，`BY_NAVCLASS` | 脚本不应直接访问 |

注意 `get_timeout` 的返回值是**方法逻辑**（乘 2），不是字段原值。

---

## 8. 数据流总图

```
                    注册期（进程启动一次）
┌──────────────────────────────────────────────────┐
│ attach_native_ptr_slot(class_body)                │
│   mParams[0] ← nativePtr (BY_NAVCLASS, 0)        │
│ attach_method × N                                 │
└──────────────────────────────────────────────────┘

                    运行期（首次带 self 的调用）
┌──────────────┐   ensure    ┌─────────────────────┐
│ VM 实例      │ ──────────► │ Box<ConfigBuiltins> │
│ nativePtr    │ ◄────────── │ （Rust 堆，权威状态）  │
│ mParams[0]   │   指针写入   │ debug, timeout_ms   │
└──────────────┘             └─────────────────────┘
                                      │
                                      ▼
                             boyia_native_ref / mut
                             你写的 impl 方法
                             self.debug / self.timeout_ms
                                      │
                                      ▼
                             GC：vtable mark / drop
```

---

## 9. 当前限制（Config 适用）

| 项 | 说明 |
|----|------|
| 带 `self` 的方法 | 仅 **sync**（`#[boyia_sync_builtin]`）；async 带 `self` 编译报错 |
| 字段类型 | `bool`、整数、浮点、`String` |
| 状态位置 | Rust `Box`，非 VM `mParams` 属性镜像 |
| 写回 | `&mut self` 直接改 `Box`，无 `boyia_store_to` |
| 脚本访问字段 | 不支持；仅通过方法 |
| 实例 | `new(Config)` 拷贝类模板；`Box` 在首次 native 调用时懒分配 |
| GC | 依赖 `NativePropTrait` vtable；对象不可达时 `native_drop` 释放 `Box` |

---

## 10. 相关文件

| 路径 | 职责 |
|------|------|
| `examples/boyia_cli/src/builtins/external/config.rs` | Config 源码 |
| `examples/boyia_cli/src/builtins/mod.rs` | `DEFAULT_BUILTINS` 注册 |
| `examples/boyia_cli/src/runner/macro/builtin_macro.rs` | `#[boyia_native_object]`、`#[boyia_class]` 过程宏 |
| `crates/boyia_gc/src/native_gc.rs` | `nativePtr` 槽、`boyia_ensure_native`、`NativePropTrait` |
| `examples/boyia_cli/src/runner/sync.rs` | `SyncCallSite`、`this_function()` |
| `examples/boyia_cli/script/main.boyia` | 脚本调用示例（`testString` 内） |
