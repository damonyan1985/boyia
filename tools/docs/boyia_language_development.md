# Boyia 语言开发文档

本文面向 Boyia 脚本开发者和 Rust 扩展开发者，说明常用语法、数组与多维数据、以及 Builtins 扩展机制。

编辑器侧（语法高亮、补全、悬停）见 [Boyia IDE 扩展 README](../plugin/README.md)。

## 1. 变量声明

Boyia 使用 `var` 声明变量：

```boyia
var name = "boyia";
var a = 100, b = 200;
var arr = ["1", "2"];
var matrix = [[1, 2, 3], [4, 5, 6]];
var obj = {
    key: "value",
    count: 3
};
```

说明：

- 支持一行声明多个变量（逗号分隔）。
- 支持字符串、数字、一维数组、嵌套数组、Map 对象等。
- 空数组写作 `[]`；多维字面量如 `[[1,2],[3,4]]` 在编译期会展开为嵌套 Array（见第 10 节）。

## 2. 类和对象

### 2.1 定义类

```boyia
class Printer {
    fun say(msg) {
        BY_Log(msg);
    }
};
```

### 2.2 创建对象

Boyia 中通过原生函数 `new` 创建对象：

```boyia
var p = new(Printer);
p.say("hello");
```

## 3. 属性、方法与属性方法

### 3.1 普通方法

```boyia
class MathUtil {
    fun add(a, b) {
        return a + b;
    }
};
```

### 3.2 属性（prop 字段）

```boyia
class User {
    prop name = "guest";
};
```

### 3.3 属性方法（prop fun）

`prop` 可以和 `fun` 组合，定义属性方法：

```boyia
class Calc {
    prop fun mul(a, b) {
        return a * b;
    }
};
```

### 3.4 异步属性方法（prop async）

```boyia
class Service {
    prop async loadAsync(url) {
        var result = (await this.loadPromise(url));
        return result;
    }
};
```

## 4. 继承

使用 `extends` 实现继承：

```boyia
class BaseLogger {
    fun log(msg) {
        BY_Log(msg);
    }
};

class AppLogger extends BaseLogger {
    prop fun info(msg) {
        this.log("[INFO] " + msg);
    }
};
```

## 5. require 与模块加载

可使用 `require` 加载脚本文件：

```boyia
require("./util/util.boyia");
```

运行时会基于**当前脚本文件**所在目录解析相对路径并编译加载目标脚本。VS Code 扩展中对 `require` 字符串提供跳转与文档链接。

## 6. Builtins 编写（Rust 侧）

Boyia 的 builtin 分为两层：

| 层级 | Crate / 目录 | 全局类 | 注册时机 |
|------|----------------|--------|----------|
| 运行时核心 | `crates/boyia_builtins` | `String`、`Map`、`Array`、`MicroTask` | `boyia_runtime` 初始化时自动注册 |
| CLI 扩展 | `examples/boyia_cli/src/builtins` | `File`、`Https`、`Zip`、`Json`、`Tensor`、`Config` 等 | CLI 启动时通过 `DEFAULT_BUILTINS` 注册 |

CLI 侧目录与入口：

| 路径 | 职责 |
|------|------|
| `builtins/utility/` | `File`、`Https`、`Zip`、`Json` |
| `builtins/external/` | 带 Rust 字段映射的扩展类（如 `Config`） |
| `builtins/ai/tensor.rs` | `Tensor` 工厂与句柄管理 |
| `builtins/mod.rs` | `DEFAULT_BUILTINS` 注册表 |
| `runner/async.rs`、`runner/sync.rs` | 异步 / 同步 native 基础设施 |
| `runner/macro/builtin_macro.rs` | 过程宏 `#[boyia_class]` 等（`boyia_cli` 的 `[lib]` proc-macro） |
| `runner/macro/builtin_json.rs` | Boyia 值 ↔ `serde_json::Value` |
| `runner/macro/builtin_vec.rs` | Boyia Array ↔ `Vec<usize>` / 嵌套 `NestedVec` |

> **说明**：`Json` 已从 `boyia_builtins` 迁出，同步与异步 JSON 能力统一由 CLI 的 `Json` 类提供；仅使用 `boyia_runtime` 而不注册 CLI builtins 时，不会有 `Json` / `Tensor` 等 CLI 类。

### 6.1 推荐写法：`#[boyia_class]` 宏

新 builtin 优先用过程宏声明，无需手写 `create_global_class` / `gen_builtin_class_function`。宏在编译期展开 native 函数、handler 以及 `registrar`。

典型结构（可参考 `file.rs`、`json.rs`、`tensor.rs`）：

```rust
use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use serde_json::Value as JsonValue;

struct FileBuiltins;

#[boyia_class(name = "File", registrar = builtin_file_class)]
impl FileBuiltins {
    #[boyia_async_builtin(method = "read")]
    fn file_read(path: String) -> AsyncBuiltinResult {
        match std::fs::read_to_string(&path) {
            Ok(text) => AsyncBuiltinResult::Ok { data: Some(text) },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.read error: {err}"),
            },
        }
    }

    #[boyia_sync_builtin(method = "isAbsolute")]
    fn file_is_absolute(path: String) -> bool {
        std::path::Path::new(&path).is_absolute()
    }
}
```

属性说明：

- `#[boyia_class(name = "ClassName", registrar = builtin_xxx_class)]`：挂在 `impl` 上，生成全局类注册函数。
- `#[boyia_async_builtin(method = "...")]`：异步方法，最后一个脚本参数为 callback。VM native 符号默认为 `{Rust 方法名}_native`（可用 `native = ...` 覆盖）。
- `#[boyia_sync_builtin(method = "...")]`：同步方法，直接在 VM 线程执行并写回结果。native 符号规则同上。
- `#[optional(default = "...")]`：可选参数，**目前仅支持 `String` 类型**（如 Tensor 的 `dtype` 默认 `"float32"`）。省略时由宏注入默认值，且不占用 VM 参数槽位。

约束：

- `impl` 内只能是带上述属性的关联函数。
- 默认情况下方法**不带 `self`**（如 `File`、`Json`）。
- 若需将 Rust struct **字段**映射为 Boyia 对象属性，并在方法里用 `&self` / `&mut self` 读写，见 **[6.6 Rust struct 字段映射](#66-rust-struct-字段映射config)**。
- 异步 work 函数返回 `AsyncBuiltinResult`（见 6.2）。
- 同步 work 函数常见返回类型见下表。

**同步参数类型（宏自动从 VM 读取）：**

| Rust 类型 | 脚本侧 |
|-----------|--------|
| `String` | 字符串 |
| `bool`、整数、浮点 | 对应标量 |
| `serde_json::Value` | Map / Array 等（经 JSON 桥转换） |
| `Vec<usize>` | 一维非负整数数组（如 shape `[2, 3]`） |
| `Vec<NestedVec>` | 嵌套数组（如 `[1,2,3]` 或 `[[1,2],[3,4]]`） |
| `Option<String>` | 可空字符串 |

**同步返回类型：**

| Rust 类型 | 脚本侧 |
|-----------|--------|
| `()`、`bool`、整数、浮点、`String` | 标量 |
| `Option<serde_json::Value>` | Boyia 对象（`Json.parse`） |
| `Option<Vec<usize>>` | Boyia Array（如 `Tensor.shape`） |
| `Handle`（`type Handle = usize`） | 无符号整数句柄；`0` 表示失败 / 无效 |

注册到 CLI：

```rust
// examples/boyia_cli/src/builtins/mod.rs
pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    external::config::builtin_config_class,
    utility::https::builtin_https_class,
    utility::file::builtin_file_class,
    utility::zip::builtin_zip_class,
    utility::json::builtin_json_class,
    ai::tensor::builtin_tensor_class,
];
```

`BoyiaRunner::create(registrars)` 会在 Boyia 任务线程上依次调用这些 `registrar`。

### 6.2 异步 Builtins：`AsyncBuiltinResult`

异步 work 函数在线程池执行，完成后切回 runtime 线程，通过 callback 投递 `Map` 结果。

```rust
pub enum AsyncBuiltinResult {
    Ok { data: Option<String> },   // status=ok，data 为字符串或省略
    OkJson(serde_json::Value),     // status=ok，data 为解析后的 Boyia 对象（Map/Array 等）
    Fail { message: String },      // status=fail，message 为错误信息
}
```

脚本侧统一收到 `{ status, data?, message? }` 结构的 Map（与 `File.read`、`Https.load` 相同）。

核心流程（实现在 `runner/async.rs`）：

1. native 入口读取业务参数与 callback。
2. 将 callback 打包为可跨线程传递的信息。
3. 在线程池执行 work 函数（IO / 网络 / JSON 等）。
4. 回到 runtime 线程构建结果 Map 并触发 callback。
5. 调用 `consume_micro_task` 驱动微任务继续执行。

脚本侧用法（与 File / Https 相同）：

```boyia
class Demo {
    prop async callAsync() {
        var result = (await this.callPromise());
        if (result.get("status") == "ok") {
            BY_Log(result.get("data"));
        } else {
            BY_Log(result.get("message"));
        }
    }

    prop async callPromise() {
        Util.newMicrotask(fun(resolve) {
            YourBuiltin.doAsync("param", resolve);
        });
    }
};
```

实现注意点：

- callback 必须在 runtime 线程触发，不要在工作线程直接回调 VM。
- 统一返回 `status` + `data` / `message`，降低脚本侧处理复杂度。
- 参数校验失败时尽早返回 `OpHandleResult::kOpResultEnd`。
- native 层保持「参数解析 + 任务投递 + 错误收敛」，业务逻辑写在 work 函数里。

### 6.3 同步 Builtins

同步方法在 VM 线程直接调用 work 函数，通过 `runner/sync.rs` 的 `SyncReturn` 将 Rust 返回值写回 VM。

常见用法：

- 读 `String` / `bool` 等标量参数，返回 `bool`、`String` 或数字（如 `File.isAbsolute`）。
- 读 Boyia 对象并返回 JSON 字符串：`Json.toString`（参数为 `serde_json::Value`）。
- 读 JSON 字符串并返回 Boyia 对象：`Json.parse`（返回 `Option<serde_json::Value>`，失败时 native 结束）。
- 读 Boyia 数组并返回 Rust `Vec` / 嵌套结构：`builtin_vec.rs`（Tensor 工厂方法）。
- 返回句柄整数：`Handle`，`0` 表示创建失败（Tensor 工厂）。

宏会为 `Option<serde_json::Value>` 调用 `builtin_json.rs` 的 `set_sync_json_return`；为 `Option<Vec<usize>>` 调用 `set_sync_vec_usize_return`。

### 6.4 Json 内置类

`Json` 同时提供同步与异步 API，Rust 实现拆为两部分：

| 文件 | 职责 |
|------|------|
| `builtins/utility/json.rs` | `#[boyia_class]` 定义：`parse`、`toString`、`asyncParse`、`asyncToString` |
| `runner/macro/builtin_json.rs` | Boyia 值 ↔ `serde_json::Value` 转换，供宏与 `AsyncBuiltinResult::OkJson` 使用 |

脚本 API：

```boyia
// 同步（VM 线程）
var obj = Json.parse(jsonText);       // JSON 字符串 → Map / Array / String / 数字 / null
var text = Json.toString(boyiaValue);   // Boyia 值 → JSON 字符串

// 异步（线程池 + callback Map）
Json.asyncParse(jsonText, resolve);     // 成功时 data 为解析后的对象
Json.asyncToString(boyiaValue, resolve); // 成功时 data 为 JSON 字符串
```

异步 JSON 解析应返回 `AsyncBuiltinResult::OkJson(value)`，以便 callback 的 `data` 字段直接是 Boyia 对象，而不是字符串。

完整示例见 `examples/boyia_cli/script/main.boyia` 中的 `jsonRead` / `jsonAsyncParse` / `jsonAsyncToString`。

### 6.5 Tensor 内置类（CLI / AI）

`Tensor` 提供类似 PyTorch 的 CPU 张量工厂，数据通过 **句柄（Handle）** 管理，脚本不直接持有 Rust 对象。

| 方法 | 说明 |
|------|------|
| `empty(shape, dtype?)` | 未初始化存储 |
| `zeros` / `ones` / `full` | 填充 0 / 1 / 指定标量 |
| `tensor(data, dtype?)` | 从嵌套数组构建（如 `[1,2,3]` 或 `[[1,2],[3,4]]`） |
| `arange` / `arangeStartEnd` / `arangeStartEndStep` | 整数序列 |
| `randn(shape, dtype?)` | 标准正态随机 |
| `shape(id)` | 返回 shape 数组；无效 id 时失败 |
| `toString(id)` | 可读摘要字符串 |
| `destroy(id)` | 释放句柄，成功返回 `true` |

**dtype**（可选，默认 `"float32"`）：`float32` / `f32`、`float64` / `f64`、`int64` / `i64`、`int32` / `i32`、`bool` 等（见 `TensorDtype::parse`）。

**句柄约定：**

- 成功创建返回 **从 1 开始的正整数**；**`0` 表示失败**（参数非法、shape 不匹配等）。
- 内部 `TensorRegistry` 用 `Vec<Option<BoyiaTensor>>` 存槽位，索引为 `handle - 1`；`destroy` 后槽位进入 `free_list` 复用。
- `tensor(data)` **总是新建**张量并分配新句柄，不会从 registry「读取已有 id」。

脚本示例（`examples/boyia_cli/script/ai.boyia`）：

```boyia
var id = Tensor.tensor([1, 2, 3]);
if (id == 0) {
    Util.log("Tensor.tensor failed");
    return;
}
Util.log(Tensor.toString(id));
var shape = Tensor.shape(id);
Util.log("rank: " + shape.size());
Tensor.destroy(id);
```

Rust 侧 `.map(store_tensor)` 是标准库 **`Option::map`**：仅当 `BoyiaTensor::from_nested` 返回 `Some` 时才写入 registry，不是张量的逐元素 `map` 运算。

### 6.6 Rust native 对象（Config）

除 **6.1** 的「仅方法、无 `self`」写法外，CLI 还支持把 Rust struct 状态放在 **`Box<T>` + `nativePtr`** 上，并在 **sync** 方法中通过 `&self` / `&mut self` 读写（如 `Config` 的 `debug`、`timeout_ms`）。

典型写法（源码见 `builtins/external/config.rs`）：

```rust
use builtin_macro::{boyia_class, boyia_native_object};

#[boyia_native_object]
pub struct ConfigBuiltins {
    #[boyia_field_default = "false"]
    debug: bool,
    #[boyia_field_default = "30000"]
    timeout_ms: u64,
}

#[boyia_class(name = "Config", registrar = builtin_config_class]
impl ConfigBuiltins {
    #[boyia_sync_builtin(method = "setTimeout")]
    fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms;
    }
}
```

脚本侧：

```boyia
var config = new(Config);
config.setTimeout(5333);
Util.log("timeout: " + config.getTimeout());
```

**完整说明**（注册流程、`nativePtr` 懒分配、`#[boyia_native_object]` / `#[boyia_class(...)]` 宏展开、方法与 GC）见专用文档：

**[Builtin Native 对象映射](./builtin_struct_fields_mapping.md)**

当前限制摘要：`native` 模式字段类型为标量（`bool`、整数、浮点、`String`）；带 `self` 的方法仅 **sync**；持久状态在 Rust `Box` 内，通过 `nativePtr` 关联实例；脚本只能通过方法访问，不能直接读写字段属性。

### 6.7 底层手写 native（可选）

若不使用宏，仍可参照 `boyia_builtins` 或 `boyia_lib`：`create_global_class` + `gen_builtin_class_function`，在 native 中手动读 local、写结果。CLI 的 File / Https / Zip / Json / Tensor 已统一改为宏写法，新扩展建议沿用 **6.1**。

手写异步 native 的骨架示意：

```rust
pub unsafe fn builtin_async_xxx(vm: *mut LVoid) -> OpHandleResult {
    // 1) 读取参数（业务参数 + callback）
    // 2) 投递线程池任务
    // 3) 完成后切回 runtime 线程执行 callback
    // 详见 runner/async.rs 中 schedule 与 build_async_result_map
    OpHandleResult::kOpResultSuccess
}
```

## 7. Rust 原生扩展函数（参照 boyia_lib）

`boyia_lib` 提供了标准 native 函数注册样例，例如：

- `create_object`（对应脚本 `new(...)`）
- `log_print`（对应脚本 `BY_Log(...)`）
- `require_file`（对应脚本 `require(...)`）

### 7.1 函数签名

native 函数通常形如：

```rust
pub unsafe fn your_native(vm: *mut LVoid) -> OpHandleResult
```

### 7.2 读取参数

通过 VM API 读取 local 参数：

- `get_local_size(vm)`：参数个数
- `get_local_value(index, vm)`：指定参数

### 7.3 注册到 Runtime

可参考 `boyia_runtime` 的 native 初始化逻辑，将函数加入 native 表：

```rust
self.append_native("yourFunc", your_native as NativePtr);
```

脚本侧即可直接调用：

```boyia
yourFunc(...);
```

## 8. async/await 机制

Boyia 已支持 `async/await` 语法，常用于异步 builtins 回调封装。

### 8.1 基本写法

```boyia
class Api {
    prop async loadAsync(url) {
        var result = (await this.loadPromise(url));
        return result;
    }

    prop async loadPromise(url) {
        Util.newMicrotask(fun(resolve) {
            Https.load(url, resolve);
        });
    }
};
```

### 8.2 使用建议

- 把 callback 形式的 builtin 包装成 `Promise` 风格函数（通过 `newMicrotask` + `resolve`）。
- 在业务函数里使用 `await`，让流程更直观。
- 统一异步返回结构（如 `status/data/message`）便于脚本侧判错。
- JSON 处理优先使用 `Json.parse` / `Json.toString`（同步）或 `Json.asyncParse` / `Json.asyncToString`（异步）；旧版 `Util.fromJson` / `Util.toJson` 若仍存在，建议逐步迁移到 `Json`。

## 9. 数组与多维字面量

- 一维：`var a = [1, 2, 3];`，元素通过 `Array.get` / `Array.size` 访问（运行时核心类）。
- 多维：`var m = [[1,2],[3,4]];`，编译为嵌套 Array；`m.get(0).get(1)` 取第 0 行第 1 列。
- 空数组：`[]` 合法，可用于占位或后续 `Array.add`。
- 作为 builtin 参数时，宏经 `builtin_vec.rs` 转为 `Vec<usize>`（shape）或 `Vec<NestedVec>`（tensor 数据）；不规则嵌套或类型混用会导致工厂返回句柄 `0`。

编译器在 `crates/boyia_vm/src/compile.rs` 的 `eval_array` 中解析 `[` … `]`；解析失败会报 syntax error（建议在扩展开发时单独跑 `cargo run -p boyia_cli` 验证脚本）。

## 10. 最小示例

```boyia
require("./util/util.boyia");

class Demo {
    prop fun run() {
        Util.log("Boyia start");
    }

    prop async runAsync(url) {
        var result = (await this.loadPromise(url));
        Util.log("result: " + result.get("status"));
    }

    prop async loadPromise(url) {
        Util.newMicrotask(fun(resolve) {
            Https.load(url, resolve);
        });
    }

    prop fun parseJson(text) {
        var obj = Json.parse(text);
        Util.log("parsed: " + Json.toString(obj));
    }

    prop fun tensorSmoke() {
        var id = Tensor.zeros([2, 3]);
        if (id != 0) {
            Util.log(Tensor.toString(id));
            Tensor.destroy(id);
        }
    }
};

var d = new(Demo);
d.run();
```

---

**延伸阅读：** [仓库 README](../../README.md) · [Boyia IDE 扩展](../plugin/README.md) · [Builtin Native 对象映射](./builtin_struct_fields_mapping.md)
