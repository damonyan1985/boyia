# Builtin Struct 字段与 Boyia Class 属性映射：技术可行性分析

本文面向 Rust 扩展开发者，分析在 `examples/boyia_cli/src/builtins` 中声明带字段的 struct，并通过过程宏将字段映射为 Boyia class 属性、在 `#[boyia_sync_builtin]` / `#[boyia_async_builtin]` 中读写并写回的技术可行性。

相关实现与用法见 [Boyia 语言开发文档](./boyia_language_development.md) 第 6 节。

## 1. 背景与目标

### 1.1 当前写法

CLI builtin 的典型结构是**空 marker struct + 纯关联函数**，例如 `json.rs`：

```rust
struct JsonBuiltins;

#[boyia_class(name = "Json", registrar = builtin_json_class)]
impl JsonBuiltins {
    #[boyia_sync_builtin(native = json_parse_native, method = "parse")]
    fn json_parse(text: String) -> Option<JsonValue> {
        serde_json::from_str(&text).ok()
    }
}
```

### 1.2 期望能力

希望支持类似下面的语义：

```rust
struct ConfigBuiltins {
    debug: bool,
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class)]
impl ConfigBuiltins {
    #[boyia_sync_builtin(native = config_set_timeout_native, method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms; // 修改写回 Boyia class 属性
    }

    #[boyia_async_builtin(native = config_load_native, method = "load")]
    fn load(&mut self, url: String) -> AsyncBuiltinResult {
        // 可读写字段，最终同步到 class 属性
        ...
    }
}
```

目标可拆解为三件事：

1. **注册期**：struct 字段 → `class_body.mParams` 初始槽位（脚本可读 `Config.debug`）
2. **调用期**：handler 从 VM 加载 → 填充 Rust struct → 调用用户函数
3. **返回期**：struct 修改 → 写回 `mParams`（脚本侧可见）

---

## 2. 现状：宏与 VM 各自做什么

### 2.1 `#[boyia_class]` 宏约束

宏实现位于 `examples/boyia_cli/src/runner/macro/builtin_macro.rs`，当前约束如下：

| 约束 | 含义 |
|------|------|
| `impl` 内只能是 builtin 函数 | 不支持字段、不支持普通方法 |
| 禁止 `self` | `collect_args` 遇到 `self` 直接报错 |
| 只生成 `attach_method` | registrar 里**不注册任何 class 属性** |
| 全局静态类 | `register_async_builtin_class` 创建 `File` / `Json` 等单例全局类 |

### 2.2 VM 层：class 属性存储

Boyia VM 中 class 属性存放在 `BoyiaFunction.mParams` 槽位中。核心 builtins 已有先例：

- **`String`**（`crates/boyia_builtins/src/string.rs`）：注册 `buffer`、`hash` 属性
- **`MicroTask`**（`crates/boyia_builtins/src/microtask.rs`）：注册 `task` 属性，并在 native 方法中读写 `mParams` 槽位

辅助函数：

- `gen_builtin_class_function`：添加 `BY_NAV_FUNC` 方法
- `gen_builtin_class_prop_function`：添加 `BY_NAV_PROP` 属性方法

**结论：VM 层完全支持 class 属性；缺的是宏层把 Rust struct 字段与这些槽位桥接起来。**

### 2.3 调用时能否拿到 class 对象（`this`）

`BY_NAV_FUNC` 静态方法调用时，VM 在进 native 前会 `local_push(this)`（`crates/boyia_vm/src/execute.rs`）：

```rust
if value_type == ValueType::BY_NAV_FUNC || value_type == ValueType::BY_NAV_PROP {
    local_push(&mut (*e_state).mStackFrame.mClass, &mut *vm);
    return nav_fun(&mut *vm);
}
```

因此 handler 可通过 `get_local_value(size - 1)` 拿到 `this`，再经 `(*obj).mValue.mObj.mPtr → BoyiaFunction → mParams` 读写属性。

现有 `SyncCallSite` / `CallSite` **尚未暴露** `this`，但基础设施足够，扩展 `this_obj()` / `this_function()` 即可。

---

## 3. 技术可行性结论

| 问题 | 结论 |
|------|------|
| struct 字段能否存为 Boyia class 属性？ | **能**，VM 已有 `mParams` 机制 |
| sync / async builtin 能否使用并写回？ | **sync 完全可行**；**async 需快照 + VM 线程写回** |
| 能否用过程宏实现？ | **能**，在现有 `builtin_macro.rs` 上扩展 |
| 改字段是否等于改属性？ | **sync 可以**（`&mut self` + load/store）；**async 为最终一致性** |

**整体判断：技术可行性高，与现有架构一致。建议以「VM mParams 为权威 + 调用期 Rust 镜像 + 宏生成 load/store」为主线。**

---

## 4. 实现方案

### 4.1 宏扩展：编译期 schema → 属性注册

`syn` 在 `#[boyia_class]` 展开时解析 struct 的 `Fields::Named`，为每个字段生成 registrar 代码：

```rust
// 宏展开示意
pub fn builtin_config_class(vm: &mut BoyiaVM, gen_id: &mut dyn FnMut(&str) -> LUintPtr) {
    register_async_builtin_class(vm, gen_id, "Config", |class_body, vm, gen_id| {
        attach_class_prop(class_body, gen_id("debug"), ValueType::BY_INT, 0);
        attach_class_prop(class_body, gen_id("timeout_ms"), ValueType::BY_INT, 0);
        attach_method(gen_id, "setTimeout", ..., class_body, vm);
    });
}
```

需新增 `attach_class_prop` helper（逻辑类似 `string.rs` 手写注册，可放到 `runner/async.rs`）。

**Phase 1 支持的字段类型**（与现有 sync 类型对齐）：

| Rust 字段类型 | VM ValueType | 双向转换 |
|---------------|--------------|----------|
| `bool` | `BY_INT` | ✅ |
| 整数（`i32` / `i64` / `u64` 等） | `BY_INT` | ✅ |
| `f32` / `f64` | `BY_REAL` | ✅ |
| `String` | `BY_STRING` / string object | ✅（需注意内存所有权） |
| `serde_json::Value` | `BY_CLASS` Map | ⚠️ 需复用 `builtin_json.rs` |

`Option<T>`、`Vec<T>`、嵌套 struct、自定义 enum 需额外序列化层，建议二期再做。

### 4.2 方法体内使用字段：三种路径

#### 方案 A：`&mut self` + 宏改写 handler（推荐）

宏保留用户函数，并生成 handler：

```rust
fn set_timeout_handler(site: &mut SyncCallSite<'_>) -> OpHandleResult {
    let this = site.this_function()?;
    let mut state = ConfigBuiltins::load_from(this)?;
    let ms = site.arg_i64(1)? as u64;
    set_timeout(&mut state, ms);
    state.store_to(this)?;
    set_sync_return((), site.vm())
}
```

- 用户体验最好
- 宏需维护「字段名 → mParams 索引」映射
- **属性必须先于方法注册**，槽位顺序固定

#### 方案 B：无 `self`，宏注入隐式 state 参数

```rust
fn set_timeout(state: &mut ConfigBuiltins, ms: u64) { ... }
```

实现更简单，但 API 不自然，不推荐作为主路径。

#### 方案 C：Rust struct 仅作 schema，运行时不实例化

宏生成局部变量与 `load_prop` / `store_prop` 调用，本质仍是方案 A。

### 4.3 同步 vs 异步写回

```
Sync Builtin（VM 线程）:
  load mParams → struct → 调用 work → store struct → mParams → 写 reg0

Async Builtin（跨线程）:
  VM 线程 load → 快照 → 线程池 work → VM 线程 before/回调 store → callback Map
```

| 场景 | 写回可行性 | 说明 |
|------|-----------|------|
| `boyia_sync_builtin` | ✅ 完全可行 | 全程 VM 线程，handler 末尾 `store_to` |
| `boyia_async_builtin` | ⚠️ 有条件可行 | work 在线程池，**不能直接碰 VM** |
| 脚本 `Config.debug = true` | ✅ 若属性在 class 上 | Rust 下次 `load` 能读到 |

**异步写回做法：**

1. **利用已有 `before` 钩子**（`boyia_async_builtin(before = ...)`）：work 前 clone 快照，work 后在 VM 线程 `store_to`
2. **扩展 `AsyncBuiltinResult`**：如 `OkWithState { data, state }`，侵入性更大

异步只读字段（如把 `timeout_ms` 传给 HTTP client）无需写回，快照只读即可。

---

## 5. 关键难点与边界

### 5.1 单一数据源（Single Source of Truth）

| 策略 | 优点 | 缺点 |
|------|------|------|
| **VM mParams 为权威**（推荐） | 脚本 `Config.timeout` 与 Rust 一致 | 每次调用 load/store 有开销 |
| **Rust `static Mutex<Struct>` 为权威** | 类似 `TensorRegistry`，异步友好 | **不是** Boyia class 属性，脚本不可见 |
| **双写** | — | 竞态、不一致，应避免 |

大对象或跨线程共享状态仍建议 **Handle + Registry**（见 `tensor.rs`）；**小标量配置类字段**适合 class 属性映射。

### 5.2 全局单例类 vs 实例类

当前 `File` / `Json` 是**全局单例 class**，属性挂在 class 对象上，全进程共享一份状态。

若需要 per-instance 状态（`new Config()`），需：

- 注册 class 模板 + `copy_object` 创建实例（类似 `MicroTask`）
- `this` 指向实例的 `mParams`，而非 class 模板

宏 API 可区分 `singleton`（默认）与 `instance` 模式。

### 5.3 属性槽位顺序

`MicroTask` 使用 `mParams.add(1)` 硬编码索引，说明**属性必须先于方法注册，且顺序固定**。宏生成顺序：

1. 所有 `attach_class_prop`
2. 所有 `attach_method`

load/store 使用编译期常量索引，避免运行时按名查找。

### 5.4 `String` 属性内存

`BY_STRING` 的 buffer 由 VM 管理；写回时需复用 `sync.rs` / `async.rs` 中已有 string 转换（如 `create_native_string`），避免悬空指针。

### 5.5 过程宏的输入形态

Rust 不允许一个 attribute 同时挂在 struct 和 impl 上。常见做法：

```rust
// 做法 1：字段宏在 struct，class 宏在 impl（已实现，见 `builtins/external/config.rs`）
#[boyia_fields]
struct ConfigBuiltins {
    #[boyia_default = "false"]
    debug: bool,
    #[boyia_default = "30000"]
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class, fields)]
impl ConfigBuiltins { ... }

// 做法 2：声明式宏合并 struct + impl
boyia_class! {
    name = "Config",
    registrar = builtin_config_class,
    struct ConfigBuiltins { debug: bool, timeout_ms: u64 }
    impl { ... }
}
```

### 5.6 与 Tensor 模式对比

`Tensor` 使用 **Handle + 全局 `TensorRegistry`**，因为状态大、生命周期复杂、不适合放进 `mParams`：

```rust
struct TensorRegistry {
    slots: Vec<Option<BoyiaTensor>>,
    free_list: Vec<usize>,
}
```

| 场景 | 推荐模式 |
|------|----------|
| 小标量配置（timeout、debug 开关） | class 属性 + struct 镜像 |
| 大张量、句柄资源 | Handle + Registry |

---

## 6. 推荐实现路线

### Phase 1 — 最小可用（仅 sync）

1. 新增 `attach_class_prop` + 按类型的 `prop_load` / `prop_store`
2. `SyncCallSite::this_function() -> *mut BoyiaFunction`
3. 扩展 `#[boyia_class]`：解析 struct 字段（或 `#[boyia_fields]`）
4. 支持 `&mut self` / `&self` 的 sync 方法；handler 自动 load/store
5. 字段类型：`bool`、整数、`String`

### Phase 2 — 异步与脚本互操作

1. async handler：调用前 load 快照，`before` 写回
2. 文档说明：async 内字段修改在 work 返回后才对脚本可见
3. 验证脚本 `Config.debug = true` 与 Rust `load` 一致

### Phase 3 — 实例类与复杂类型

1. `instance = true` 模式 + factory 方法
2. `serde_json::Value` 属性
3. 可选：`#[boyia_prop(readonly)]`、变更通知

---

## 7. 宏展开示例（端到端）

### 7.1 用户编写

```rust
#[boyia_fields]
struct HttpBuiltins {
    default_timeout: u64,
}

#[boyia_class(name = "Https", registrar = builtin_https_class)]
impl HttpBuiltins {
    #[boyia_sync_builtin(native = https_get_timeout_native, method = "getTimeout")]
    fn get_timeout(&self) -> u64 {
        self.default_timeout
    }

    #[boyia_sync_builtin(native = https_set_timeout_native, method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.default_timeout = ms;
    }
}
```

### 7.2 宏生成（概念）

```rust
impl HttpBuiltins {
    const PROP_DEFAULT_TIMEOUT: usize = 0;

    unsafe fn load_from(this: *mut BoyiaFunction) -> Self {
        Self {
            default_timeout: prop_load_u64(this, Self::PROP_DEFAULT_TIMEOUT),
        }
    }

    unsafe fn store_to(&self, this: *mut BoyiaFunction) {
        prop_store_u64(this, Self::PROP_DEFAULT_TIMEOUT, self.default_timeout);
    }
}

pub fn builtin_https_class(vm: &mut BoyiaVM, gen_id: &mut dyn FnMut(&str) -> LUintPtr) {
    register_async_builtin_class(vm, gen_id, "Https", |class_body, vm, gen_id| {
        attach_class_prop_int(class_body, gen_id("default_timeout"), 30);
        attach_method(gen_id, "getTimeout", https_get_timeout_native, class_body, vm);
        attach_method(gen_id, "setTimeout", https_set_timeout_native, class_body, vm);
    });
}

fn https_set_timeout_handler(site: &mut SyncCallSite<'_>) -> OpHandleResult {
    let this = site.this_function()?;
    let mut state = unsafe { HttpBuiltins::load_from(this) };
    let ms = some_or_end!(site.arg_i64(1)) as u64;
    https_set_timeout(&mut state, ms);
    unsafe { state.store_to(this) };
    set_sync_return((), site.vm())
}
```

---

## 8. 相关文件索引

| 路径 | 职责 |
|------|------|
| `examples/boyia_cli/src/runner/macro/builtin_macro.rs` | `#[boyia_class]`、`#[boyia_sync_builtin]`、`#[boyia_async_builtin]` |
| `examples/boyia_cli/src/runner/sync.rs` | 同步 native 基础设施 |
| `examples/boyia_cli/src/runner/async.rs` | 异步 native、`register_async_builtin_class`、`attach_method` |
| `crates/boyia_builtins/src/lib.rs` | `gen_builtin_class_function`、`gen_builtin_class_prop_function` |
| `crates/boyia_builtins/src/string.rs` | class 属性注册示例（`buffer`、`hash`） |
| `crates/boyia_builtins/src/microtask.rs` | 运行时读写 `mParams` 示例 |
| `examples/boyia_cli/src/builtins/ai/tensor.rs` | Handle + Registry 模式（对比参考） |
| `tools/docs/boyia_language_development.md` | Builtins 编写总览 |

---

## 9. 已实现 API（Phase 1）

| 宏 / 项 | 说明 |
|---------|------|
| `#[boyia_fields]` | 挂在 struct 上；生成 `boyia_attach_class_props`、`boyia_load_from`、`boyia_store_to` |
| `#[boyia_default = "..."]` | 字段默认值（字符串字面量，由宏解析为 bool / 整数 / 浮点 / 空字符串） |
| `#[boyia_class(..., fields)]` | `fields` 标志：注册属性槽位；sync 方法可用 `&self` / `&mut self` |
| `runner/class_props.rs` | `attach_class_prop_*`、`prop_load_*`、`prop_store_*` |
| `SyncCallSite::this_function()` | 读取 `BY_NAV_FUNC` 的 `this` 对象 |

**限制（当前）：**

- 仅 **sync** 方法支持 `self`；async 带 `self` 会在编译期报错
- 字段类型：`bool`、整数、浮点、`String`
- 示例类：`Config`（`getDebug` / `setDebug` / `getTimeout` / `setTimeout`）

## 10. 总结

在 builtins 中为 struct 定义字段，并通过扩展宏映射到 Boyia class 属性、在 sync/async builtin 中透明读写，**在架构上是可行且与 VM 设计一致的**。核心设计选择：

1. **VM `mParams` 为唯一权威状态**，Rust struct 为每次调用的临时镜像
2. **宏在 registrar 注册属性、在 handler 生成 load/store**
3. **sync 全链路在 VM 线程**；**async 用快照 + `before` 写回**
4. **大对象继续用 Handle 模式**，不与 class 标量属性混用

建议从 Phase 1（sync + 标量字段）做 POC，再逐步覆盖异步与实例类。
