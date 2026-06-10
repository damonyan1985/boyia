# Boyia 语言开发文档

本文面向 Boyia 脚本开发者和 Rust 扩展开发者，说明常用语法与扩展机制。

## 1. 变量声明

Boyia 使用 `var` 声明变量：

```boyia
var name = "boyia";
var a = 100, b = 200;
var arr = ["1", "2"];
var obj = {
    key: "value",
    count: 3
};
```

说明：

- 支持一行声明多个变量（逗号分隔）。
- 支持字符串、数字、数组、Map 对象等。

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

运行时会基于当前脚本路径解析相对路径并编译加载目标脚本。

## 6. Builtins 编写（Rust 侧）

Boyia 的 builtin 分为两层：

| 层级 | Crate / 目录 | 全局类 | 注册时机 |
|------|----------------|--------|----------|
| 运行时核心 | `crates/boyia_builtins` | `String`、`Map`、`Array`、`MicroTask` | `boyia_runtime` 初始化时自动注册 |
| CLI 扩展 | `examples/boyia_cli/src/builtins` | `File`、`Https`、`Zip`、`Json` | CLI 启动时通过 `DEFAULT_BUILTINS` 注册 |

CLI 侧示例与注册入口：

- 业务定义：`examples/boyia_cli/src/builtins/utility/`（如 `file.rs`、`json.rs`）
- 注册表：`examples/boyia_cli/src/builtins/mod.rs` 中的 `DEFAULT_BUILTINS`
- 异步/同步基础设施：`examples/boyia_cli/src/runner/async.rs`、`sync.rs`
- 过程宏：`examples/boyia_cli/src/runner/macro/builtin_macro.rs`
- JSON 转换辅助：`examples/boyia_cli/src/runner/macro/builtin_json.rs`

> **说明**：`Json` 已从 `boyia_builtins` 迁出，同步与异步 JSON 能力统一由 CLI 的 `Json` 类提供；仅使用 `boyia_runtime` 而不注册 CLI builtins 时，不会有 `Json` 类。

### 6.1 推荐写法：`#[boyia_class]` 宏

新 builtin 优先用过程宏声明，无需手写 `create_global_class` / `gen_builtin_class_function`。宏在编译期展开 native 函数、handler 以及 `registrar`。

典型结构（可参考 `file.rs`、`json.rs`）：

```rust
use crate::runner::r#async::AsyncBuiltinResult;
use builtin_macro::boyia_class;
use serde_json::Value as JsonValue;

struct FileBuiltins;

#[boyia_class(name = "File", registrar = builtin_file_class)]
impl FileBuiltins {
    #[boyia_async_builtin(native = file_read_native, method = "read")]
    fn file_read(path: String) -> AsyncBuiltinResult {
        match std::fs::read_to_string(&path) {
            Ok(text) => AsyncBuiltinResult::Ok { data: Some(text) },
            Err(err) => AsyncBuiltinResult::Fail {
                message: format!("File.read error: {err}"),
            },
        }
    }

    #[boyia_sync_builtin(native = file_is_absolute_native, method = "isAbsolute")]
    fn file_is_absolute(path: String) -> bool {
        std::path::Path::new(&path).is_absolute()
    }
}
```

属性说明：

- `#[boyia_class(name = "ClassName", registrar = builtin_xxx_class)]`：挂在 `impl` 上，生成全局类注册函数。
- `#[boyia_async_builtin(native = ..., method = "...")]`：异步方法，最后一个脚本参数为 callback。
- `#[boyia_sync_builtin(native = ..., method = "...")]`：同步方法，直接在 VM 线程执行并写回结果。

约束：

- `impl` 内只能是带上述属性的关联函数，不能有 `self`。
- 异步 work 函数返回 `AsyncBuiltinResult`（见下节）。
- 同步 work 函数返回 `()`、`bool`、整数、浮点、`String`、`Option<String>`，或 `Option<serde_json::Value>`（用于 `Json.parse` 等需返回 Boyia 对象的场景）。
- 同步/异步参数支持 `String`；需要 Boyia Map/Array 等 JSON 值时，参数类型写 `serde_json::Value`（宏会从 VM 参数自动转换）。

注册到 CLI：

```rust
// examples/boyia_cli/src/builtins/mod.rs
pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    utility::https::builtin_https_class,
    utility::file::builtin_file_class,
    utility::zip::builtin_zip_class,
    utility::json::builtin_json_class,
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
- native 层保持“参数解析 + 任务投递 + 错误收敛”，业务逻辑写在 work 函数里。

### 6.3 同步 Builtins

同步方法在 VM 线程直接调用 work 函数，通过 `runner/sync.rs` 的 `SyncReturn` 将 Rust 返回值写回 VM。

常见用法：

- 读 `String` / `bool` 等标量参数，返回 `bool`、`String` 或数字（如 `File.isAbsolute`）。
- 读 Boyia 对象并返回 JSON 字符串：`Json.toString`（参数为 `serde_json::Value`）。
- 读 JSON 字符串并返回 Boyia 对象：`Json.parse`（返回 `Option<serde_json::Value>`，失败时 native 结束）。

宏会为 `Option<serde_json::Value>` 返回值调用 `runner/macro/builtin_json.rs` 中的 `set_sync_json_return`，将 JSON 转为 Boyia Map/Array 等。

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

### 6.5 底层手写 native（可选）

若不使用宏，仍可参照 `boyia_builtins` 或 `boyia_lib` 的方式：`create_global_class` + `gen_builtin_class_function`，在 native 中手动读 local、写结果。CLI 的 File/Https/Zip/Json 已统一改为宏写法，新扩展建议沿用 **6.1** 的模式。

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

## 9. 最小示例

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
};

var d = new(Demo);
d.run();
```

---

