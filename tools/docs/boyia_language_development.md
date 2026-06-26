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

### 3.5 匿名函数（`fun`）

Boyia 使用 `fun(参数) { ... }` 声明匿名函数，常用于 builtin 的 **callback** 最后一个参数，例如：

```boyia
File.read("path.txt", fun(res) {
    BY_Log(res.get("data"));
});
```

**匿名函数必须在类方法中使用**（类内的 `fun` 或 `prop fun`）。不要在顶层脚本、普通全局 `fun` 里把匿名函数当作 callback 传给 builtin。

原因：创建匿名函数时 VM 需要当前类的上下文（`mClass`）。只有从类实例方法调用时该上下文才会正确设置；写在普通 `fun` 里会导致匿名函数无法绑定对象。

违反此规则时，运行时会报错并停止执行，例如：

```text
runtime error (line 5): anonymous function (fun) must be used inside a class method; declare it within prop fun or fun of a class
```

```boyia
// ❌ 不推荐：顶层 fun 里传 callback
fun runServer() {
    server.receive(fun(port, msg) {
        server.send(port, msg);
    });
}

// ✅ 推荐：写在类方法里，通过 this 访问实例
class WsEchoServer {
    prop server;

    prop fun run(host, port) {
        this.server = new(WebSocketServer);
        this.server.start(host, port);
        while (this.server.isRunning() == 1) {
            this.server.receive(fun(port, msg) {
                this.server.send(port, "echo: " + msg);
            });
        }
    }
};
```

适用场景：

- **异步 builtin**（`File.read`、`Https.load` 等）：最后一个参数为 callback。
- **返回元组的同步 builtin**（如 `WebSocketServer.receive`）：Rust 侧返回 `(port, message)` 等元组，脚本最后一个参数为 callback，元组字段按顺序成为 callback 的参数（见 [6.1](#61-推荐写法boyia_class-宏)）。

类方法内的匿名函数可捕获外层 `this`，在 callback 里访问实例字段与方法。

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

路径相对于**当前被编译脚本文件**所在目录解析（绝对路径按平台规则直接 canonicalize）。VS Code 扩展中对 `require` 字符串提供跳转与文档链接。

### 5.1 编译与执行两阶段（CLI）

`boyia_cli` 把两件事分开做：

1. **编译**：读入口脚本，以及它 `require` 到的所有文件，把它们翻译成 VM 能执行的指令；**此阶段不运行**脚本里的 `BY_Log`、赋值等顶层语句。
2. **执行**：按约定好的顺序，依次运行每个文件顶层的代码（类定义、全局变量等）。

```bash
cd examples/boyia_cli
cargo run    # 内部顺序：先编译全部文件，再统一执行
```

自己嵌入 VM 时，也应先编完所有相关文件，再开始执行。

### 5.2 字面量 require（编译期处理）

写法固定为 `require("路径")`，路径必须是**写在引号里的字符串**，不能是变量：

```boyia
require("./util/util.boyia");
```

这种写法在**编译当前文件时**就会处理，不会在运行时再调一次 `require` 函数。具体会做两件事：

1. **记下依赖**：根据当前文件位置，算出 `./util/util.boyia` 的绝对路径，加入「待编译文件列表」。
2. **稍后一起编**：当前文件编完后，再按依赖关系去编列表里的文件——**被 require 的文件会先编、先执行**，引用它的文件后编、后执行。入口 `main.boyia` 通常最后执行。

因此脚本里写 `require("./util/util.boyia")` 时，并不会立刻运行 `util.boyia` 里的代码；要等整个工程编译完，进入执行阶段后，才会按上面的顺序跑起来。

**常见情况：**

| 情况 | 说明 |
|------|------|
| A require B，B require C | 执行顺序大致为 C → B → A（先底层，后上层） |
| 多个文件 require 同一个 util | util 只编译、只执行一次 |
| 文件互相 require（循环） | 不会无限编译；但环里谁先谁后不保证，应避免顶层代码互相依赖 |
| `require(变量)` | 见下一节，走运行时逻辑 |

### 5.3 动态 require（运行时处理）

路径来自变量或表达式时，只能在**程序已经跑起来之后**再加载：

```boyia
var path = "./util/util.boyia";
require(path);
```

此时会：**当场**读取路径 → 编译该文件（及其依赖）→ **马上执行**新编出来的顶层代码，以便后面的语句能用到新注册的类或函数。

已打包成 bundle / exe、不再从磁盘读源码时，运行时的 `require` 会被忽略。

### 5.4 路径相对谁算？

| 什么时候 | 相对路径基于 |
|----------|----------------|
| 编译某个 `.boyia` 文件时的 `require("...")` | **这个文件**所在目录 |
| CLI 指定的入口脚本 | 入口文件的目录（启动时登记） |
| 运行时的 `require(变量)` | 当前上下文或入口脚本目录 |

注意：`File.read("a.txt")` 等 API 默认相对**进程当前工作目录**，和 `require` 规则不同；需要时可 `OS.chdir` 改工作目录。

## 6. Builtins 编写（Rust 侧）

Boyia 的 builtin 分为两层：

| 层级 | Crate / 目录 | 全局类 | 注册时机 |
|------|----------------|--------|----------|
| 运行时核心 | `crates/boyia_builtins` | `String`、`Map`、`Array`、`MicroTask` | `boyia_runtime` 初始化时自动注册 |
| CLI 扩展 | `examples/boyia_cli/src/builtins` | `File`、`Https`、`Zip`、`Json`、`OS`、`Tensor`、`Config` 等 | CLI 启动时通过 `DEFAULT_BUILTINS` 注册 |

CLI 侧目录与入口：

| 路径 | 职责 |
|------|------|
| `builtins/utility/` | `File`、`Https`、`Zip`、`Json`、`OS` |
| `builtins/external/` | 带 `nativePtr` 堆状态的扩展类（如 `Config`，见 6.7） |
| `builtins/ai/tensor.rs` | `Tensor` 工厂与句柄管理 |
| `builtins/mod.rs` | `DEFAULT_BUILTINS` 注册表 |
| `runner/async.rs`、`runner/sync.rs` | 异步 / 同步 native 基础设施 |
| `runner/macro/builtin_macro.rs` | 过程宏 `#[boyia_class]` 等（`boyia_cli` 的 `[lib]` proc-macro） |
| `runner/macro/builtin_json.rs` | Boyia 值 ↔ `serde_json::Value` |
| `runner/macro/builtin_vec.rs` | Boyia Array ↔ `Vec<usize>` / 嵌套 `NestedVec` |

> **说明**：`Json` 已从 `boyia_builtins` 迁出，同步与异步 JSON 能力统一由 CLI 的 `Json` 类提供；仅使用 `boyia_runtime` 而不注册 CLI builtins 时，不会有 `Json` / `Tensor` 等 CLI 类。

### 6.0 运行 boyia_cli

`examples/boyia_cli` 提供通用命令行入口（`src/main.rs`、`src/cli.rs`）：

```bash
cd examples/boyia_cli
cargo run                          # 读 .boyia_rc
cargo run -- script/ai.boyia       # 指定脚本（存在则优先）
cargo run -- --help
```

**入口脚本解析：**

1. 命令行路径（文件须存在；不存在则回退 `.boyia_rc`）
2. `.boyia_rc` 查找顺序：工程根 → 可执行文件目录 → 用户主目录
3. `.boyia_rc` 内容：`script=path/to/entry.boyia`（也支持 `entry=`、`main=`）

`File.*` 等使用相对路径的 builtin 以**进程 cwd** 为准；可用 `OS.cwd()` / `OS.chdir(path)` 调整工作目录。

CLI 启动流程：`BoyiaRunner::compile_file` 编译入口及 `require` 依赖图 → `run_exe_file` 执行全局 entry（见 [§5 require 与模块加载](#5-require-与模块加载)）。

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
- `#[boyia_async_builtin(method = "...")]`：异步方法，最后一个脚本参数为 callback。VM native 符号默认为 `{Rust 方法名}_native`（可用 `native = ...` 覆盖）。callback 须写在类方法中（见 [3.5](#35-匿名函数fun)）。
- `#[boyia_sync_builtin(method = "...")]`：同步方法，直接在 VM 线程执行并写回结果。native 符号规则同上。
  - 若 Rust 返回**非空元组**（如 `(u16, String)`），脚本侧**最后一个参数必须是 callback**；宏自动捕获 callback 并用元组各字段作为 callback 参数调用，Rust work 函数**不要**声明 callback 参数。匿名 callback 须写在类方法中（见 [3.5 匿名函数](#35-匿名函数fun)）。
- `#[optional(default = "...")]`：可选参数，**目前仅支持 `String` 类型**（如 Tensor 的 `dtype` 默认 `"float32"`）。省略时由宏注入默认值，且不占用 VM 参数槽位。

约束：

- `impl` 内只能是带上述属性的关联函数。
- 默认情况下方法**不带 `self`**（如 `File`、`Json`、`OS`）。
- 若需在 **sync** 方法里用 `&self` / `&mut self` 读写 Rust 堆上状态，在 struct 上加 `#[boyia_native_object]`，见 **[6.7 Rust native 对象（Config）](#67-rust-native-对象config)**。
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
| 非空元组（如 `(u16, String)`） | 脚本侧**不**直接接收返回值；须传 callback，字段依次作为 callback 参数（见 [3.5](#35-匿名函数fun)） |

注册到 CLI：

```rust
// examples/boyia_cli/src/builtins/mod.rs
pub const DEFAULT_BUILTINS: &[BuiltinRegistrar] = &[
    external::config::builtin_config_class,
    utility::https::builtin_https_class,
    utility::file::builtin_file_class,
    utility::os::builtin_os_class,
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

### 6.5 OS 内置类

`OS` 提供进程级环境查询与 cwd 控制（`builtins/utility/os.rs`）。`File.*` 等使用相对路径的 builtin 以**进程当前工作目录**解析路径。

| 方法 | 返回 | 说明 |
|------|------|------|
| `OS.cwd()` | `String` | 当前工作目录 |
| `OS.chdir(path)` | `bool` | 切换 cwd，成功为 `true` |
| `OS.name()` | `String` | 平台名（如 `linux`、`macos`、`windows`） |
| `OS.cpuCount()` | 数字 | 可用并行度（逻辑 CPU 数） |

```boyia
BY_Log(OS.cwd());
OS.chdir("/tmp");
BY_Log(OS.name());
BY_Log(OS.cpuCount());
```

### 6.6 Tensor 内置类（CLI / AI）

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

### 6.7 Rust native 对象（Config）

除 **6.1** 的「仅方法、无 `self`」写法外，CLI 还支持把 Rust struct 状态放在 **`Box<T>` + `nativePtr`** 上，并在 **sync** 方法中通过 `&self` / `&mut self` 读写（如 `Config` 的 `debug`、`timeout_ms`）。

**不再需要** `#[boyia_class(..., native)]`：`impl` 里只要有带 `self` 的 sync 方法，宏会要求 struct 已实现 `NativePropTrait`（由 `#[boyia_native_object]` 提供），并自动挂 `nativePtr`。

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

#[boyia_class(name = "Config", registrar = builtin_config_class)]
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

当前限制摘要：字段类型为标量（`bool`、整数、浮点、`String`）；带 `self` 的方法仅 **sync**；持久状态在 Rust `Box` 内，通过 `nativePtr` 关联实例；脚本只能通过方法访问，不能直接读写字段属性；`#[boyia_class]` 不再接受 `native` 参数（由 `#[boyia_native_object]` 自动推断）。

### 6.8 底层手写 native（可选）

若不使用宏，仍可参照 `boyia_builtins` 或 `boyia_lib`：`create_global_class` + `gen_builtin_class_function`，在 native 中手动读 local、写结果。CLI 的 File / Https / Zip / Json / OS / Tensor 已统一改为宏写法，新扩展建议沿用 **6.1**。

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

`boyia_lib` 里注册了脚本最常用的几个底层能力：

| 脚本里写的 | 何时生效 | Rust 实现 | 作用 |
|------------|----------|-----------|------|
| `new(类)` | 运行时 | `create_object` | 创建对象，把结果交给表达式 |
| `BY_Log(...)` | 运行时 | `log_print` | 打印，无返回值 |
| `require("...")` | **编译时** | `require_file_compile` | 记下还要编译哪个文件（见 §5.2） |
| `require(变量)` | 运行时 | `require_file` | 当场编译并执行该文件（见 §5.3） |

下面分「运行时函数」和「编译时函数」说明扩展方式。

### 7.1 运行时函数（Native）

脚本执行到调用时才会进入 Rust，签名一般为：

```rust
pub unsafe fn your_native(vm: &mut BoyiaVM) -> OpHandleResult
```

**怎么把结果还给脚本？** VM **不会**自动帮你填返回值。若函数要参与表达式（例如 `var x = foo()`），需在 Rust 里显式写入表达式结果槽：

- `set_native_result(&mut value, vm)` — 写入任意 Boyia 值
- `set_int_result(n, vm)` — 写入整数

只做事、不返回值的函数（如 `BY_Log`）可以不写。若在表达式里调用了这类函数，返回值不可信。

**怎么读参数？**

- `get_local_size(vm)` — 参数个数
- `get_local_value(index, vm)` — 第几个参数（`require_file` 的路径在 index 0）

**注册：**

```rust
self.append_native("yourFunc", your_native as NativePtr);
```

### 7.2 编译时函数（CompileFunction）

这类函数在**翻译脚本、生成指令**的阶段执行，程序还没开始跑。

可以做两类事：

1. **编译时算出常量**  
   例如 `myDouble(21)` 在编译阶段就算成 `42`，生成的指令里直接是数字 `42`，运行时不再调用 `myDouble`。

2. **编译时登记后续工作**  
   例如字面量 `require("./a.boyia")`：此时**不执行** `a.boyia`，只把「这个文件也要参与本次编译」记下来，等当前文件编完再去编它（详见 §5.2）。  
   以前文档里说的「编译副作用」，指的就是这种：**在编译阶段改变「还要编哪些文件」等状态，而不是给表达式一个可用的值**。

编译时函数的 Rust 签名：

```rust
pub type CompileFunction = unsafe fn(&CompileArgs) -> CompileArg;
```

- **参数** `CompileArgs`：只能是字面量（字符串、整数、浮点、`true`/`false`），并带上 VM 指针以便访问 Runtime。
- **返回值** `CompileArg`：告诉编译器「表达式这边应该当成什么常量」。

| 返回值 | 含义 | 编译器生成的代码 |
|--------|------|------------------|
| `Void` | 没有表达式结果（如 `require` 只登记文件） | 不生成赋值指令 |
| `Int` / `Real` / `Bool` | 数值或布尔常量 | 把常量写入表达式结果槽 |
| `Str` | 字符串常量 | 生成取字符串常量的指令 |

**注册：**

```rust
self.append_compile_native("myDouble", my_double_compile as CompileFunction);
```

**示例：编译时把参数乘 2**

```rust
use boyia_vm::{CompileArg, CompileArgs};

pub unsafe fn my_double_compile(args: &CompileArgs) -> CompileArg {
    let Some(n) = args.int(0) else {
        return CompileArg::Void;
    };
    CompileArg::Int(n * 2)
}
```

脚本写 `var x = myDouble(21);` 时，效果接近直接写 `var x = 42;`。

**`require` 的编译时实现** `require_file_compile` 返回 `Void`：只登记路径，不向表达式提供值，也不生成运行时的 `require` 调用。

编译器遇到标识符时，**先查编译时函数表，再查运行时函数表**；同名时编译时版本优先（所以 `require("...")` 不会变成运行时 `require`）。

### 7.3 给 Runtime 加能力时要动哪里

扩展编译时 `require` 或自定义编译时函数时，通常会用到：

- `Runtime::enqueue_compile_script` — 把「待编译文件路径」登记进去（`require` 用这个）
- `BoyiaCompileInfo`（`boyia_runtime/src/info.rs`）— 记录已编文件、待编列表，并决定「先编依赖、后编引用方」的顺序

实现细节（函数表名、`pending_requires`、`move_entries_to_end` 等）见源码注释，日常使用脚本一般不必关心。

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
