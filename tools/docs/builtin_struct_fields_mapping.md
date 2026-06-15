# Builtin 字段与 Boyia 对象映射

本文以 `examples/boyia_cli/src/builtins/external/config.rs` 中的 **Config** 内置类为例，说明 Rust struct 字段如何映射为 Boyia 对象属性，以及 `#[boyia_fields]` / `#[boyia_class]` 的完整使用与宏展开流程。

相关总览见 [Boyia 语言开发文档](./boyia_language_development.md) 第 6 节。

---

## 1. 源码：你写什么

```rust
// examples/boyia_cli/src/builtins/external/config.rs

use builtin_macro::{boyia_class, boyia_fields};

#[boyia_fields]
pub struct ConfigBuiltins {
    #[boyia_field_default = "false"]
    debug: bool,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class, fields)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(native = config_get_debug_native, method = "getDebug")]
    fn get_debug(&self) -> bool {
        self.debug
    }

    #[boyia_sync_builtin(native = config_set_debug_native, method = "setDebug")]
    fn set_debug(&mut self, value: bool) {
        self.debug = value;
    }

    #[boyia_sync_builtin(native = config_get_timeout_native, method = "getTimeout")]
    fn get_timeout(&self) -> u64 {
        self.timeout_ms * 2
    }

    #[boyia_sync_builtin(native = config_set_timeout_native, method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
}
```

三个宏分工：

| 宏 | 挂在哪 | 作用 |
|----|--------|------|
| `#[boyia_fields]` | `struct ConfigBuiltins` | 把字段注册为 Boyia 属性，并生成 `boyia_load_from` / `boyia_store_to` |
| `#[boyia_field_default = "..."]` | 字段上 | 注册时的默认值 |
| `#[boyia_class(name = "Config", registrar = ..., fields)]` | `impl` 上 | 注册 Boyia 类与方法；`fields` 开启字段 load/store |
| `#[boyia_sync_builtin(...)]` | 方法上 | 把 Rust 方法映射为脚本可调用的同步 native 方法 |

### 1.1 `#[boyia_field_default]` 支持的字段类型

属性写法统一为 **字符串字面量**：`#[boyia_field_default = "..."]`。宏按字段的 Rust 类型解析引号内的文本。

| Rust 字段类型 | `boyia_field_default` 示例 | 省略时的默认值 | 说明 |
|---------------|------------------------------|----------------|------|
| `bool` | `#[boyia_field_default = "true"]` | `false` | 仅 `"true"` 为 true，其余为 false |
| `String` | `#[boyia_field_default = "production"]` | `""`（空字符串） | 引号内为 Boyia 属性初始文本 |
| `f32` / `f64` | `#[boyia_field_default = "1.5"]` | `0.0` | 按浮点解析 |
| `u64` / `usize` | `#[boyia_field_default = "30000"]` | `0` | 按无符号整数解析 |
| `i8` / `i16` / `i32` / `i64` / `isize` | `#[boyia_field_default = "-1"]` | `0` | 按有符号整数解析 |
| `u8` / `u16` / `u32` | `#[boyia_field_default = "255"]` | `0` | 存入 VM `BY_INT` 槽位 |

示例（含 `String` 字段）：

```rust
#[boyia_fields]
pub struct AppBuiltins {
    #[boyia_field_default = "development"]
    env: String,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}
```

未列出的类型（如 `Option<T>`、`Vec<T>`、自定义 struct）暂不支持，编译期会报错。

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
    ConfigBuiltins::boyia_attach_class_props(...)   // 先挂字段属性
    attach_method(..., "getDebug",  ...)
    attach_method(..., "setDebug",  ...)
    attach_method(..., "getTimeout", ...)
    attach_method(..., "setTimeout", ...)
})
```

注册完成后，VM 里存在全局类 **Config**：

- **属性**（来自 struct 字段）：`debug`、`timeout_ms`（存在 `mParams` 槽位 0、1）
- **方法**（来自 `#[boyia_sync_builtin]`）：`getDebug`、`setDebug`、`getTimeout`、`setTimeout`

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
  → 实例 mParams 带上 debug=false、timeout_ms=30000（默认值）

config.setTimeout(5333)
  → 调用 native config_set_timeout_native
  → Rust handler：load → set_timeout → store

config.getTimeout()
  → 调用 native config_get_timeout_native
  → Rust handler：load → get_timeout（返回 timeout_ms * 2）→ 不 store
  → 日志打印 10666（5333 * 2）
```

说明：

- **方法**（`setTimeout` / `getTimeout`）是脚本主要入口。
- **字段**（`debug`、`timeout_ms`）挂在对象 `mParams` 上；Rust 方法通过 `self.debug` / `self.timeout_ms` 读写。脚本也可按属性名访问（如 `config.timeout_ms`），但命名是 Rust 字段名（蛇形），与方法名（驼峰）不同。

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
  ① class_body = site.this_function()     // 拿到 this 的 mParams 容器
  ② state = ConfigBuiltins::boyia_load_from(class_body)
       // 新建临时 struct：{ debug: ..., timeout_ms: 5333 之前的值 }
  ③ ms = site.arg_i64(1) as u64           // 从 VM 读脚本参数 5333
  ④ ConfigBuiltins::set_timeout(&mut state, ms)
       // 执行你写的：self.timeout_ms = ms
  ⑤ state.boyia_store_to(class_body, vm)  // 写回 mParams[1]
  ⑥ set_sync_return((), vm)               // 返回给脚本
                             ▼
┌─────────────────────────────────────────────────────────────┐
│ config 实例上 timeout_ms 属性 = 5333                         │
└─────────────────────────────────────────────────────────────┘
```

`get_timeout`（`&self`）流程相同，但**没有第 ⑤ 步**——只读，不写回 VM。

---

## 5. `#[boyia_fields]` 宏展开流程

**输入**（你写的 struct）：

```rust
#[boyia_fields]
pub struct ConfigBuiltins {
    #[boyia_field_default = "false"]
    debug: bool,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}
```

**宏在编译期做的事：**

1. 解析每个字段的名字、类型、`#[boyia_field_default]`
2. 按声明顺序分配固定槽位索引：`debug → 0`，`timeout_ms → 1`
3. 生成 `impl ConfigBuiltins { ... }` 辅助方法

**展开结果（概念代码，省略属性清理）：**

```rust
pub struct ConfigBuiltins {
    debug: bool,
    timeout_ms: u64,
}

impl ConfigBuiltins {
    pub const BOYIA_FIELD_DEBUG: usize = 0;
    pub const BOYIA_FIELD_TIMEOUT_MS: usize = 1;

    /// 注册期：把字段挂到 class_body.mParams
    pub unsafe fn boyia_attach_class_props(
        class_body: *mut BoyiaFunction,
        vm: &mut BoyiaVM,
        gen_id: &mut dyn FnMut(&str) -> LUintPtr,
    ) {
        class_props::attach_class_prop_bool(class_body, gen_id("debug"), false);
        class_props::attach_class_prop_i64(class_body, gen_id("timeout_ms"), 30000);
    }

    /// 调用期：VM 属性 → 临时 Rust struct（每次调用新建一个）
    pub unsafe fn boyia_load_from(class_body: *mut BoyiaFunction) -> Self {
        Self {
            debug: class_props::prop_load_bool(class_body, Self::BOYIA_FIELD_DEBUG),
            timeout_ms: class_props::prop_load_u64(class_body, Self::BOYIA_FIELD_TIMEOUT_MS),
        }
    }

    /// 调用期：临时 Rust struct → VM 属性（仅 &mut self 方法结束后调用）
    pub unsafe fn boyia_store_to(
        &self,
        class_body: *mut BoyiaFunction,
        vm: &mut BoyiaVM,
    ) {
        class_props::prop_store_bool(class_body, Self::BOYIA_FIELD_DEBUG, self.debug);
        class_props::prop_store_u64(class_body, Self::BOYIA_FIELD_TIMEOUT_MS, self.timeout_ms);
    }
}
```

要点：

- **持久状态在 VM 的 `mParams`**，不在 Rust 堆里长期保存 `ConfigBuiltins`。
- **`boyia_load_from` 每次调用都会 `Self { ... }` 新建一个临时 struct**。
- 宏**不会改写**你方法体里的 `self.timeout_ms`，那就是普通 Rust 字段访问。

---

## 6. `#[boyia_class(..., fields)]` 宏展开流程

**输入**（你写的 impl）：

```rust
#[boyia_class(name = "Config", registrar = builtin_config_class, fields)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(native = config_set_timeout_native, method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
    // ... 其余方法同理
}
```

**宏在编译期对每个 `#[boyia_sync_builtin]` 方法：**

1. 识别 `&self` / `&mut self` / 无 self
2. 保留方法体，重新输出到 `impl ConfigBuiltins { ... }`
3. 生成 `{方法名}_handler` + `define_sync_native!`
4. 生成 `builtin_config_class` 注册函数

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
    let mut state = unsafe { ConfigBuiltins::boyia_load_from(class_body) };

    let ms = some_or_end!(site.arg_i64(1)) as u64;

    let __sync_result = ConfigBuiltins::set_timeout(&mut state, ms);

    unsafe { state.boyia_store_to(class_body, site.vm()); }

    crate::runner::sync::set_sync_return(__sync_result, site.vm())
}

// C. 注册为 native 函数
unsafe fn config_set_timeout_native(vm: &mut BoyiaVM) -> OpHandleResult {
    sync_dispatch(vm, 2, set_timeout_handler)  // min_locals = 参数数 + 1
}

// D. 类注册器
pub fn builtin_config_class(
    vm: &mut BoyiaVM,
    gen_id: &mut dyn FnMut(&str) -> LUintPtr,
) {
    register_async_builtin_class(vm, gen_id, "Config", |class_body, vm, gen_id| {
        unsafe { ConfigBuiltins::boyia_attach_class_props(class_body, vm, gen_id); }
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
    let mut state = unsafe { ConfigBuiltins::boyia_load_from(class_body) };

    let __sync_result = ConfigBuiltins::get_timeout(&state);
    // 无 boyia_store_to

    set_sync_return(__sync_result, site.vm())
}
```

`ConfigBuiltins::set_timeout(&mut state, ms)` 与 `state.set_timeout(ms)` 完全等价；宏选用关联函数写法，因为 handler 里已有名为 `state` 的局部变量。

---

## 7. 字段 vs 方法：分别映射到什么

| Rust 侧 | Boyia 侧 | 脚本示例 |
|---------|----------|----------|
| 字段 `debug` | 属性 `debug`（`mParams[0]`） | `config.debug`（属性名，蛇形） |
| 字段 `timeout_ms` | 属性 `timeout_ms`（`mParams[1]`） | `config.timeout_ms` |
| 方法 `get_debug` | 方法 `getDebug` | `config.getDebug()` |
| 方法 `set_debug` | 方法 `setDebug` | `config.setDebug(true)` |
| 方法 `get_timeout` | 方法 `getTimeout` | `config.getTimeout()` → 返回 `timeout_ms * 2` |
| 方法 `set_timeout` | 方法 `setTimeout` | `config.setTimeout(5333)` |

注意 `get_timeout` 的返回值是**方法逻辑**（乘 2），不是字段原值。字段原值存在 `timeout_ms` 属性上。

---

## 8. 数据流总图

```
                    注册期（进程启动一次）
┌──────────────────────────────────────────────────┐
│ ConfigBuiltins::boyia_attach_class_props          │
│   mParams[0] ← debug      (default false)          │
│   mParams[1] ← timeout_ms (default 30000)        │
│ attach_method × 4                                 │
└──────────────────────────────────────────────────┘

                    运行期（每次方法调用）
┌──────────────┐    load     ┌─────────────────┐    store    ┌──────────────┐
│ VM mParams   │ ──────────► │ ConfigBuiltins  │ ──────────► │ VM mParams   │
│ (权威存储)   │             │ (临时栈上副本)   │  仅 &mut self │ (权威存储)   │
└──────────────┘             └─────────────────┘             └──────────────┘
                                      │
                                      ▼
                             你写的 impl 方法
                             self.debug / self.timeout_ms
```

---

## 9. 当前限制（Config 适用）

| 项 | 说明 |
|----|------|
| 带 `self` 的方法 | 仅 **sync**（`#[boyia_sync_builtin]`）；async 带 `self` 编译报错 |
| 字段类型 | `bool`、整数、浮点、`String` |
| 写回时机 | 仅 `&mut self` 的 sync 方法在返回前 `boyia_store_to` |
| 临时 struct | 每次调用 `boyia_load_from` 新建，不跨调用复用 |
| 实例 | `new(Config)` 从类模板拷贝属性；`this` 指向该实例的 `mParams` |

---

## 10. 相关文件

| 路径 | 职责 |
|------|------|
| `examples/boyia_cli/src/builtins/external/config.rs` | Config 源码 |
| `examples/boyia_cli/src/builtins/mod.rs` | `DEFAULT_BUILTINS` 注册 |
| `examples/boyia_cli/src/runner/macro/builtin_macro.rs` | `#[boyia_fields]`、`#[boyia_class]` 过程宏 |
| `examples/boyia_cli/src/runner/class_props.rs` | 属性槽位 attach / load / store |
| `examples/boyia_cli/src/runner/sync.rs` | `SyncCallSite`、`this_function()` |
| `examples/boyia_cli/script/main.boyia` | 脚本调用示例（`testString` 内） |
