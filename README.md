# Boyia Language

中国人自己开发的 OOP 脚本语言引擎，基于纯 Rust 实现，支持类、继承、异步与 Rust 原生扩展。可作为嵌入式脚本引擎，在 CLI 中已集成 `File`、`Https`、`Zip`、`Json` 等内置能力。

Boyia is a Rust-only OOP scripting language engine with custom syntax, class-based OOP, async/await, and native extensions written in Rust.

## 环境要求

- [Rust](https://www.rust-lang.org/tools/install)（stable，含 `cargo`）
- 网络访问（首次构建会拉取依赖；运行 `Https` 示例需联网）

克隆仓库后，在项目根目录操作：

```bash
git clone <your-repo-url>
cd boyia
```

## 快速运行 boyia_cli

`boyia_cli` 是官方示例运行器：启动 VM、注册 CLI 内置类、编译并执行 Boyia 脚本。

```bash
# 在 examples/boyia_cli 目录（推荐）
cd examples/boyia_cli
cargo run

# 指定脚本路径（文件存在时优先使用）
cargo run -- script/ai.boyia

# 命令行路径不存在时，回退到 .boyia_rc
cargo run -- missing.boyia

# 无参数：从 .boyia_rc 读取入口脚本
cargo run
```

**脚本解析顺序：**

1. 命令行传入的路径（且文件存在）
2. 否则按顺序查找 `.boyia_rc`：
   - 工程根目录（从 cwd 向上，含 `Cargo.toml` 或 `.git` 的目录）
   - 可执行文件同级目录
   - 用户主目录（如 `/Users/<you>/.boyia_rc`）

`.boyia_rc` 示例（`examples/boyia_cli/.boyia_rc`）：

```ini
script=script/main.boyia
```

相对路径相对于 `.boyia_rc` 所在目录。详见 `examples/boyia_cli/src/cli.rs`。

成功时终端会打印 `Boyia CLI: <脚本绝对路径>`，随后输出脚本日志。

**编译与执行：** CLI 先 `compile_file` 编译入口及全部 `require` 依赖（仅生成字节码），再 `run_exe_file` 执行；字面量 `require("./x.boyia")` 在编译期解析并入队，依赖模块按后序顺序先于入口执行。详见 [开发文档 §5](tools/docs/boyia_language_development.md#5-require-与模块加载)。

### 调试选项

跳过 CLI 扩展内置类（仅保留 runtime 自带的 `String` / `Map` / `Array` / `MicroTask`），用于排查初始化问题：

```bash
# Linux / macOS
BOYIA_INIT_MINIMAL=1 cargo run -p boyia_cli

# Windows PowerShell
$env:BOYIA_INIT_MINIMAL="1"; cargo run -p boyia_cli
```

## 新人上手：编写第一个 Demo

默认入口由 `examples/boyia_cli/.boyia_rc` 配置为 `script/main.boyia`。也可通过命令行指定其它脚本。

### 1. 最小 Hello World

将 `examples/boyia_cli/script/main.boyia` 暂时替换为：

```boyia
require("./util/util.boyia");

class Hello {
    prop fun run() {
        Util.log("Hello, Boyia!");
    }
};

var app = new(Hello);
app.run();
```

然后运行：

```bash
cd examples/boyia_cli
cargo run
```

应看到类似 `< Hello, Boyia! >` 的日志。

### 2. 使用类、Map 与内置 Json

```boyia
require("./util/util.boyia");

class Demo {
    prop fun run() {
        var user = {
            name: "boyia",
            score: 100
        };
        Util.log("name = " + user.name);

        var jsonText = "{\"ok\":true}";
        var obj = Json.parse(jsonText);
        Util.log("parsed ok = " + obj.ok);
        Util.log("back to json: " + Json.toString(obj));
    }
};

new(Demo).run();
```

### 3. 异步示例（File / Https）

完整异步、`async/await` 写法见仓库自带的 `main.boyia`（含 `File.read`、`Json.asyncParse`、`Https.load` 等）。模式一般是：

1. 用 `Util.newMicrotask` 包装 callback；
2. 调用 `File.read(path, resolve)` 等异步内置方法；
3. 在 `prop async` 方法里 `await` 结果 Map（`status` / `data` / `message`）。

```boyia
// 回调形态（节选）
File.read("vm_test.json", fun(res) {
    if (res.get("status") == "ok") {
        Util.log(res.get("data"));
    }
});
```

`vm_test.json` 已放在 `examples/boyia_cli/` 目录，可与 `File.read` 联调。

### 4. 模块引用

脚本可通过 `require` 加载其它 `.boyia` 文件：

```boyia
require("./util/util.boyia");
```

- **字面量路径**（`require("./x.boyia")`）：编译期解析并入队，与入口一并编译；执行时依赖模块的全局代码先于入口运行。
- **动态路径**（`require(path)`）：运行时再编译并立即执行新模块的顶层代码。

路径相对于**当前被编译脚本文件**所在目录解析（与 `File.read` 使用的进程 cwd 不同）。

## CLI 内置类一览

| 类 | 说明 | 典型方法 |
|----|------|----------|
| `File` | 异步文件 IO | `read`, `write`, `exists`, `create`, `delete` … |
| `Https` | 异步 HTTP | `load`, `request` |
| `Zip` | 异步压缩/解压 | `compress`, `extract` |
| `Json` | JSON 同步 + 异步 | `parse`, `toString`, `asyncParse`, `asyncToString` |
| `OS` | 进程环境 | `cwd`, `chdir`, `name`, `cpuCount` |
| `Tensor` | CPU 张量（AI 示例） | `empty`, `zeros`, `ones`, `randn`, `id` … |
| `Config` | 带 Rust 堆状态的示例类 | `getDebug`, `setDebug`, `getTimeout`, `setTimeout` |

Runtime 启动时还会自动注册：`String`、`Map`、`Array`、`MicroTask`（见 `crates/boyia_builtins`）。

## 仓库结构

```
boyia/
├── crates/
│   ├── boyia_vm/          # 虚拟机、编译与执行（含 CompileFunction / CompileArg）
│   ├── boyia_runtime/   # Runtime、native 表、compile 表、编译流水线（DFS require）
│   ├── boyia_builtins/  # 核心内置类（String/Map/Array/MicroTask）
│   ├── boyia_lib/       # 通用 native 与编译期函数（new、BY_Log、require）
│   └── ...
├── examples/
│   ├── boyia_cli/       # ★ 推荐入口：运行脚本 + CLI 内置类
│   │   ├── .boyia_rc          # 默认入口脚本配置
│   │   ├── script/main.boyia
│   │   └── src/builtins/      # utility/（File、OS…）、external/（Config）
│   └── boyia_lsp/       # .boyia 语言的 LSP 服务（可选）
└── tools/docs/
    ├── boyia_language_development.md   # 语法与扩展开发文档
    └── builtin_struct_fields_mapping.md  # native 对象（#[boyia_native_object]）详解
```

## 用 Rust 扩展 Boyia

- **CLI 内置类**：在 `examples/boyia_cli/src/builtins/` 用 `#[boyia_class]`、`#[boyia_async_builtin]`、`#[boyia_sync_builtin]` 声明；带实例状态时在 struct 上加 `#[boyia_native_object]`（无需 `#[boyia_class(..., native)]`），并加入 `builtins/mod.rs` 的 `DEFAULT_BUILTINS`。
- **通用 native 函数**：参考 `crates/boyia_lib`，在 `init_native_function` 中注册；有返回值时需 `set_native_result` / `set_int_result` 写入 `reg0`。
- **编译期函数**：`CompileFunction` + `CompileArg` 返回值，在 `init_compile_function` 注册；可用于常量折叠或编译期副作用（如字面量 `require`）。见开发文档 §7.2。
- **JSON 转换辅助**：`examples/boyia_cli/src/runner/macro/builtin_json.rs`。

详细步骤、宏约束与异步返回约定见开发文档。

## 其他命令

```bash
# 只检查 boyia_cli 能否编译
cargo check -p boyia_cli

# 构建 release 版 CLI
cargo build -p boyia_cli --release

# 语言服务（编辑器集成，可选）
cargo run -p boyia_lsp
```

## 文档

- [Boyia 语言开发文档](tools/docs/boyia_language_development.md) — 语法、`class`/`prop`/`async`、CLI 运行方式、`require` 编译/执行两阶段、CompileFunction、Builtins 编写、`Json` 与 `async/await` 示例
- [Builtin Native 对象映射](tools/docs/builtin_struct_fields_mapping.md) — `#[boyia_native_object]`、`nativePtr` 与带实例状态的 builtin 类

## Features

1. 支持 Rust 原生扩展：Builtins 类、Native 函数与编译期 CompileFunction 均可接入。
2. 支持面向对象：`class`、继承、`prop` 属性与方法。
3. 支持异步：`async/await` + 线程池异步 Builtins（File / Https / Zip / Json 等）。
4. 多文件模块：`require` 编译期依赖图（后序 DFS）+ 编译/执行分离。
